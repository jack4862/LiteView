use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use crate::loader::{self, LoadError, LoadOptions, LoadedImage};

pub const MIN_WORKERS: usize = 2;
pub const MAX_WORKERS: usize = 4;

type ThumbnailLoader =
    dyn Fn(&Path, &LoadOptions) -> Result<LoadedImage, LoadError> + Send + Sync + 'static;

#[derive(Debug, Clone)]
pub struct ThumbnailConfig {
    pub worker_count: usize,
    pub queue_capacity: usize,
    pub max_preview_side: u32,
    pub max_source_pixels: u64,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        let worker_count = thread::available_parallelism()
            .map(|count| count.get().clamp(MIN_WORKERS, MAX_WORKERS))
            .unwrap_or(MIN_WORKERS);
        Self {
            worker_count,
            queue_capacity: 64,
            max_preview_side: 320,
            max_source_pixels: 150_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailPriority {
    Visible,
    Prefetch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    Queued,
    Deduplicated,
    Promoted,
    QueueFull,
}

#[derive(Debug)]
pub struct ThumbnailResponse {
    pub path: PathBuf,
    pub generation: u64,
    pub result: Result<LoadedImage, LoadError>,
}

pub struct ThumbnailService {
    shared: Arc<Shared>,
    responses: Receiver<ThumbnailResponse>,
    worker_count: usize,
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    queue_capacity: usize,
    options: LoadOptions,
}

struct State {
    visible: VecDeque<Job>,
    prefetch: VecDeque<Job>,
    pending: HashMap<PathBuf, PendingJob>,
    next_id: u64,
    active: usize,
    shutdown: bool,
}

#[derive(Clone)]
struct Job {
    id: u64,
    path: PathBuf,
    state: Arc<JobState>,
}

struct JobState {
    generation: AtomicU64,
    cancelled: AtomicBool,
}

struct PendingJob {
    id: u64,
    priority: ThumbnailPriority,
    stage: JobStage,
    state: Arc<JobState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobStage {
    Queued,
    Active,
}

impl ThumbnailService {
    pub fn new(config: ThumbnailConfig) -> io::Result<Self> {
        Self::new_with_loader(config, |path, options| loader::load(path, options))
    }

    /// Creates a service with a custom loader. This is primarily useful for
    /// deterministic scheduler tests; production callers should use `new`.
    #[doc(hidden)]
    pub fn new_with_loader<F>(config: ThumbnailConfig, load: F) -> io::Result<Self>
    where
        F: Fn(&Path, &LoadOptions) -> Result<LoadedImage, LoadError> + Send + Sync + 'static,
    {
        let worker_count = config.worker_count.clamp(MIN_WORKERS, MAX_WORKERS);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                visible: VecDeque::new(),
                prefetch: VecDeque::new(),
                pending: HashMap::new(),
                next_id: 0,
                active: 0,
                shutdown: false,
            }),
            ready: Condvar::new(),
            queue_capacity: config.queue_capacity.max(1),
            options: LoadOptions {
                max_preview_side: config.max_preview_side.max(1),
                max_pixels: config.max_source_pixels.max(1),
            },
        });
        let (response_tx, response_rx) = mpsc::channel();
        let load: Arc<ThumbnailLoader> = Arc::new(load);
        let mut workers = Vec::with_capacity(worker_count);

        for index in 0..worker_count {
            let worker_shared = Arc::clone(&shared);
            let responses = response_tx.clone();
            let load = Arc::clone(&load);
            match thread::Builder::new()
                .name(format!("liteview-thumbnail-{index}"))
                .spawn(move || worker_loop(&worker_shared, &*load, &responses))
            {
                Ok(worker) => workers.push(worker),
                Err(error) => {
                    shared.shutdown();
                    for worker in workers {
                        let _ = worker.join();
                    }
                    return Err(error);
                }
            }
        }

        drop(workers);
        Ok(Self {
            shared,
            responses: response_rx,
            worker_count,
        })
    }

    pub fn request(
        &self,
        path: impl Into<PathBuf>,
        generation: u64,
        priority: ThumbnailPriority,
    ) -> RequestStatus {
        let path = path.into();
        let mut state = lock(&self.shared.state);

        if let Some(existing) = state.pending.get(&path) {
            let existing_generation = existing.state.generation.load(Ordering::Acquire);
            let existing_id = existing.id;
            let existing_priority = existing.priority;
            let existing_stage = existing.stage;
            let existing_state = Arc::clone(&existing.state);

            if generation > existing_generation {
                existing_state
                    .generation
                    .store(generation, Ordering::Release);
                existing_state.cancelled.store(false, Ordering::Release);
            }

            if existing_stage == JobStage::Queued
                && priority == ThumbnailPriority::Visible
                && existing_priority == ThumbnailPriority::Prefetch
                && let Some(position) = state.prefetch.iter().position(|job| job.id == existing_id)
            {
                let job = state
                    .prefetch
                    .remove(position)
                    .expect("located prefetch job");
                state.visible.push_back(job);
                if let Some(existing) = state.pending.get_mut(&path) {
                    existing.priority = ThumbnailPriority::Visible;
                }
                drop(state);
                self.shared.ready.notify_one();
                return RequestStatus::Promoted;
            }

            return RequestStatus::Deduplicated;
        }

        if state.visible.len() + state.prefetch.len() >= self.shared.queue_capacity {
            if priority == ThumbnailPriority::Visible {
                let Some(evicted) = state.prefetch.pop_back() else {
                    return RequestStatus::QueueFull;
                };
                evicted.state.cancelled.store(true, Ordering::Release);
                state.pending.remove(&evicted.path);
            } else {
                return RequestStatus::QueueFull;
            }
        }

        state.next_id = state.next_id.wrapping_add(1);
        let job = Job {
            id: state.next_id,
            path: path.clone(),
            state: Arc::new(JobState {
                generation: AtomicU64::new(generation),
                cancelled: AtomicBool::new(false),
            }),
        };
        state.pending.insert(
            path,
            PendingJob {
                id: job.id,
                priority,
                stage: JobStage::Queued,
                state: Arc::clone(&job.state),
            },
        );
        match priority {
            ThumbnailPriority::Visible => state.visible.push_back(job),
            ThumbnailPriority::Prefetch => state.prefetch.push_back(job),
        }
        drop(state);
        self.shared.ready.notify_one();
        RequestStatus::Queued
    }

    /// Cancels all queued or active work for a directory generation.
    pub fn cancel_generation(&self, generation: u64) -> usize {
        self.cancel_matching(|candidate, _| candidate == generation)
    }

    /// Cancels one path regardless of its generation.
    pub fn cancel_path(&self, path: impl AsRef<Path>) -> bool {
        self.cancel_matching(|_, candidate| candidate == path.as_ref()) > 0
    }

    /// Keeps matching-generation work only for paths the caller still needs.
    /// Call this when the viewport or selection changes.
    pub fn retain(&self, generation: u64, paths: &HashSet<PathBuf>) -> usize {
        self.cancel_matching(|candidate, path| candidate == generation && !paths.contains(path))
    }

    /// Polls one response. Callers that own a generation should prefer
    /// `poll_for_generation`, which drains stale responses for them.
    pub fn poll(&self) -> Option<ThumbnailResponse> {
        self.responses.try_recv().ok()
    }

    /// Returns the next response for `generation` and discards all stale ones
    /// encountered first.
    pub fn poll_for_generation(&self, generation: u64) -> Option<ThumbnailResponse> {
        loop {
            let response = self.poll()?;
            if response.generation == generation {
                return Some(response);
            }
        }
    }

    pub fn is_pending(&self, path: impl AsRef<Path>) -> bool {
        lock(&self.shared.state).pending.contains_key(path.as_ref())
    }

    pub fn queued_len(&self) -> usize {
        let state = lock(&self.shared.state);
        state.visible.len() + state.prefetch.len()
    }

    pub fn active_len(&self) -> usize {
        lock(&self.shared.state).active
    }

    pub fn pending_len(&self) -> usize {
        lock(&self.shared.state).pending.len()
    }

    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    fn cancel_matching<F>(&self, matches: F) -> usize
    where
        F: Fn(u64, &Path) -> bool,
    {
        let mut state = lock(&self.shared.state);
        let State {
            visible,
            prefetch,
            pending,
            ..
        } = &mut *state;
        let visible = take_matching_queue(visible, pending, &matches);
        let prefetch = take_matching_queue(prefetch, pending, &matches);
        let active = pending
            .iter()
            .filter(|(path, pending)| {
                pending.stage == JobStage::Active
                    && matches(pending.state.generation.load(Ordering::Acquire), path)
                    && !pending.state.cancelled.swap(true, Ordering::AcqRel)
            })
            .count();
        visible + prefetch + active
    }
}

impl Drop for ThumbnailService {
    fn drop(&mut self) {
        self.shared.shutdown();
    }
}

impl Shared {
    fn shutdown(&self) {
        let mut state = lock(&self.state);
        state.shutdown = true;
        for pending in state.pending.values() {
            pending.state.cancelled.store(true, Ordering::Release);
        }
        state.visible.clear();
        state.prefetch.clear();
        state.pending.clear();
        self.ready.notify_all();
    }
}

fn worker_loop(shared: &Shared, load: &ThumbnailLoader, responses: &Sender<ThumbnailResponse>) {
    while let Some(job) = next_job(shared) {
        let result = load(&job.path, &shared.options);
        let latest_generation = job.state.generation.load(Ordering::Acquire);
        let cancelled = job.state.cancelled.load(Ordering::Acquire);

        let shutdown = {
            let mut state = lock(&shared.state);
            if state
                .pending
                .get(&job.path)
                .is_some_and(|pending| pending.id == job.id)
            {
                state.pending.remove(&job.path);
            }
            state.active = state.active.saturating_sub(1);
            state.shutdown
        };

        if !cancelled && !shutdown {
            let _ = responses.send(ThumbnailResponse {
                path: job.path,
                generation: latest_generation,
                result,
            });
        }
    }
}

fn next_job(shared: &Shared) -> Option<Job> {
    let mut state = lock(&shared.state);
    loop {
        if state.shutdown {
            return None;
        }
        if let Some(job) = state
            .visible
            .pop_front()
            .or_else(|| state.prefetch.pop_front())
        {
            let pending = state
                .pending
                .get(&job.path)
                .filter(|pending| pending.id == job.id && pending.stage == JobStage::Queued);
            if pending.is_none() {
                continue;
            }
            if job.state.cancelled.load(Ordering::Acquire) {
                state.pending.remove(&job.path);
                continue;
            }
            if let Some(pending) = state.pending.get_mut(&job.path) {
                pending.stage = JobStage::Active;
            }
            state.active += 1;
            return Some(job);
        }
        state = shared
            .ready
            .wait(state)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

fn take_matching_queue<F>(
    queue: &mut VecDeque<Job>,
    pending: &mut HashMap<PathBuf, PendingJob>,
    matches: &F,
) -> usize
where
    F: Fn(u64, &Path) -> bool,
{
    let mut kept = VecDeque::with_capacity(queue.len());
    let mut cancelled = 0;
    for job in queue.drain(..) {
        if matches(job.state.generation.load(Ordering::Acquire), &job.path) {
            job.state.cancelled.store(true, Ordering::Release);
            pending.remove(&job.path);
            cancelled += 1;
        } else {
            kept.push_back(job);
        }
    }
    *queue = kept;
    cancelled
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

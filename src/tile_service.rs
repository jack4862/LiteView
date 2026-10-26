use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use crate::tile::TileCoord;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TileRequestKey {
    pub generation: u64,
    pub path: PathBuf,
    pub coord: TileCoord,
}

#[derive(Debug, Clone)]
pub struct TileDecodeRequest {
    pub path: PathBuf,
    pub source_dimensions: [u32; 2],
    pub source_rect: [u32; 4],
    pub output_size: [u32; 2],
    pub exif_orientation: u8,
    pub icc_profile: Option<Arc<[u8]>>,
}

pub trait TileDecoder: Send + Sync + 'static {
    fn decode(&self, request: &TileDecodeRequest) -> Result<Vec<u8>, String>;
}

struct WorkItem {
    key: TileRequestKey,
    request: TileDecodeRequest,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct TileResponse {
    pub key: TileRequestKey,
    pub result: Result<Vec<u8>, String>,
    pub elapsed: Duration,
}

pub struct TileService {
    requests: SyncSender<WorkItem>,
    responses: Receiver<TileResponse>,
    pending: HashMap<TileRequestKey, Arc<AtomicBool>>,
}

impl TileService {
    pub fn new(decoder: Arc<dyn TileDecoder>) -> Result<Self, String> {
        let cpu_count = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(2);
        Self::new_with_worker_count(decoder, cpu_count.saturating_sub(1).clamp(1, 4))
    }

    pub fn new_with_worker_count(
        decoder: Arc<dyn TileDecoder>,
        worker_count: usize,
    ) -> Result<Self, String> {
        Self::new_with_options(decoder, worker_count, 64)
    }

    pub fn new_with_options(
        decoder: Arc<dyn TileDecoder>,
        worker_count: usize,
        queue_capacity: usize,
    ) -> Result<Self, String> {
        let (request_tx, request_rx) = mpsc::sync_channel::<WorkItem>(queue_capacity.max(1));
        let (response_tx, response_rx) = mpsc::channel::<TileResponse>();
        let request_rx = Arc::new(Mutex::new(request_rx));
        let mut started = 0;

        for index in 0..worker_count.max(1) {
            let requests = Arc::clone(&request_rx);
            let responses = response_tx.clone();
            let decoder = Arc::clone(&decoder);
            if thread::Builder::new()
                .name(format!("liteview-tile-{index}"))
                .spawn(move || {
                    loop {
                        let item = {
                            let Ok(receiver) = requests.lock() else {
                                break;
                            };
                            let Ok(item) = receiver.recv() else {
                                break;
                            };
                            item
                        };
                        if item.cancel.load(Ordering::Relaxed) {
                            continue;
                        }
                        let started = Instant::now();
                        let result = decoder.decode(&item.request);
                        if item.cancel.load(Ordering::Relaxed) {
                            continue;
                        }
                        let _ = responses.send(TileResponse {
                            key: item.key,
                            result,
                            elapsed: started.elapsed(),
                        });
                    }
                })
                .is_ok()
            {
                started += 1;
            }
        }

        if started == 0 {
            return Err("could not start any tile workers".to_owned());
        }
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
            pending: HashMap::new(),
        })
    }

    pub fn request(&mut self, request: TileDecodeRequest, key: TileRequestKey) -> bool {
        if self.pending.contains_key(&key) {
            return true;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let item = WorkItem {
            key: key.clone(),
            request,
            cancel: Arc::clone(&cancel),
        };
        if self.requests.try_send(item).is_err() {
            return false;
        }
        self.pending.insert(key, cancel);
        true
    }

    pub fn cancel_before(&mut self, generation: u64) {
        self.pending.retain(|key, cancel| {
            if key.generation < generation {
                cancel.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    }

    pub fn cancel_path_and_older(&mut self, path: &Path, generation: u64) {
        self.pending.retain(|key, cancel| {
            if key.generation < generation || key.path != *path {
                cancel.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    }

    pub fn retain_path_coords(
        &mut self,
        path: &Path,
        generation: u64,
        visible: &HashSet<TileCoord>,
    ) -> usize {
        let mut cancelled = 0;
        self.pending.retain(|key, cancel| {
            let keep =
                key.path == path && key.generation == generation && visible.contains(&key.coord);
            if !keep {
                cancel.store(true, Ordering::Relaxed);
                cancelled += 1;
            }
            keep
        });
        cancelled
    }

    pub fn is_pending(&self, key: &TileRequestKey) -> bool {
        self.pending.contains_key(key)
    }

    pub fn poll(&mut self) -> Option<TileResponse> {
        let response = self.responses.try_recv().ok()?;
        self.pending.remove(&response.key);
        Some(response)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn pending_coords(&self, path: &Path, generation: u64) -> HashSet<TileCoord> {
        self.pending
            .keys()
            .filter(|key| key.generation == generation && key.path == *path)
            .map(|key| key.coord)
            .collect()
    }
}

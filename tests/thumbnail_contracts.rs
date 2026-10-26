use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

use image::{Rgba, RgbaImage};
use liteview::loader::{LoadError, LoadOptions, LoadedImage};
use tempfile::tempdir;

use liteview::loader;

#[path = "../src/thumbnail_service.rs"]
mod thumbnail_service;

use thumbnail_service::{
    RequestStatus, ThumbnailConfig, ThumbnailPriority, ThumbnailResponse, ThumbnailService,
};

struct LoadGate {
    entered: Arc<(Mutex<usize>, Condvar)>,
    released: Arc<(Mutex<bool>, Condvar)>,
}

impl LoadGate {
    fn new() -> Self {
        Self {
            entered: Arc::new((Mutex::new(0), Condvar::new())),
            released: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn loader(
        &self,
    ) -> impl Fn(&Path, &LoadOptions) -> Result<LoadedImage, LoadError> + Send + Sync + 'static
    {
        let entered = Arc::clone(&self.entered);
        let released = Arc::clone(&self.released);
        move |path, options| {
            let (lock, ready) = &*entered;
            *lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
            ready.notify_all();

            let (lock, ready) = &*released;
            let mut open = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while !*open {
                open = ready
                    .wait(open)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            drop(open);

            loader::load(path, options)
        }
    }

    fn wait_until_entered(&self, expected: usize, timeout: Duration) -> bool {
        wait_until(timeout, || self.entered_count() >= expected)
    }

    fn entered_count(&self) -> usize {
        *self
            .entered
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn release(&self) {
        let (lock, ready) = &*self.released;
        *lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        ready.notify_all();
    }
}

impl Drop for LoadGate {
    fn drop(&mut self) {
        self.release();
    }
}

fn service(gate: &LoadGate, queue_capacity: usize) -> ThumbnailService {
    ThumbnailService::new_with_loader(
        ThumbnailConfig {
            worker_count: 2,
            queue_capacity,
            max_preview_side: 32,
            max_source_pixels: 1_000_000,
        },
        gate.loader(),
    )
    .expect("create thumbnail service")
}

fn write_png(path: &Path) {
    RgbaImage::from_pixel(4, 3, Rgba([24, 80, 144, 255]))
        .save_with_format(path, image::ImageFormat::Png)
        .expect("write PNG fixture");
}

fn response_for(
    service: &ThumbnailService,
    generation: u64,
    timeout: Duration,
) -> Option<ThumbnailResponse> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(response) = service.poll_for_generation(generation) {
            return Some(response);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    condition()
}

fn wait_idle(service: &ThumbnailService) {
    assert!(
        wait_until(Duration::from_secs(5), || {
            service.active_len() == 0 && service.queued_len() == 0 && service.pending_len() == 0
        }),
        "thumbnail service did not become idle"
    );
}

#[test]
fn deduplicates_the_same_path_before_starting_another_job() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("duplicate.png");
    write_png(&path);
    let gate = LoadGate::new();
    let service = service(&gate, 8);

    assert_eq!(
        service.request(path.clone(), 7, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    assert!(service.is_pending(&path));
    assert!(gate.wait_until_entered(1, Duration::from_secs(2)));
    assert_eq!(
        service.request(path.clone(), 7, ThumbnailPriority::Visible),
        RequestStatus::Deduplicated
    );
    thread::sleep(Duration::from_millis(30));
    assert_eq!(gate.entered_count(), 1);
    assert_eq!(service.active_len(), 1);
    assert!(service.is_pending(&path));

    gate.release();
    let response = response_for(&service, 7, Duration::from_secs(2)).expect("deduplicated result");
    assert_eq!(response.path, path);
    assert_eq!(response.generation, 7);
    assert_eq!(
        response.result.expect("load thumbnail").preview_dimensions,
        [4, 3]
    );
    wait_idle(&service);
    assert!(!service.is_pending(path));
    thread::sleep(Duration::from_millis(30));
    assert!(service.poll().is_none());
}

#[test]
fn default_loader_produces_a_thumbnail_for_the_app() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("default-loader.png");
    write_png(&path);
    let service = ThumbnailService::new(ThumbnailConfig {
        worker_count: 2,
        queue_capacity: 8,
        max_preview_side: 2,
        max_source_pixels: 1_000_000,
    })
    .expect("create default thumbnail service");

    assert_eq!(service.worker_count(), 2);
    assert_eq!(
        service.request(path.clone(), 3, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    let response = response_for(&service, 3, Duration::from_secs(2)).expect("default response");
    let image = response.result.expect("load through app loader");
    assert_eq!(image.preview_dimensions, [2, 2]);
}

#[test]
fn discards_a_result_outside_the_requested_generation() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("stale.png");
    write_png(&path);
    let gate = LoadGate::new();
    let service = service(&gate, 8);

    assert_eq!(
        service.request(path, 10, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    assert!(gate.wait_until_entered(1, Duration::from_secs(2)));
    gate.release();
    wait_idle(&service);

    assert!(service.poll_for_generation(11).is_none());
    assert!(service.poll().is_none());
}

#[test]
fn cancellation_suppresses_active_and_queued_results() {
    let directory = tempdir().expect("temporary directory");
    let first = directory.path().join("first.png");
    let second = directory.path().join("second.png");
    let queued = directory.path().join("queued.png");
    for path in [&first, &second, &queued] {
        write_png(path);
    }
    let gate = LoadGate::new();
    let service = service(&gate, 8);

    assert_eq!(
        service.request(first.clone(), 1, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    assert_eq!(
        service.request(second.clone(), 1, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    assert!(gate.wait_until_entered(2, Duration::from_secs(2)));
    assert_eq!(
        service.request(queued.clone(), 1, ThumbnailPriority::Prefetch),
        RequestStatus::Queued
    );
    assert!(service.cancel_path(&queued));
    assert_eq!(service.cancel_generation(1), 2);
    gate.release();
    wait_idle(&service);

    assert!(service.poll().is_none());
}

#[test]
fn bounds_the_queue_and_prioritizes_visible_work() {
    let directory = tempdir().expect("temporary directory");
    let paths: Vec<_> = ["first", "second", "third", "replacement"]
        .into_iter()
        .map(|name| {
            let path = directory.path().join(format!("{name}.png"));
            write_png(&path);
            path
        })
        .collect();
    let gate = LoadGate::new();
    let service = service(&gate, 1);

    assert_eq!(
        service.request(paths[0].clone(), 2, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    assert!(wait_until(Duration::from_secs(2), || service.active_len() == 1));
    assert_eq!(
        service.request(paths[1].clone(), 2, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    assert!(wait_until(Duration::from_secs(2), || service.active_len() == 2));
    assert_eq!(
        service.request(paths[2].clone(), 2, ThumbnailPriority::Prefetch),
        RequestStatus::Queued
    );
    assert_eq!(
        service.request(paths[2].clone(), 2, ThumbnailPriority::Visible),
        RequestStatus::Promoted
    );
    assert_eq!(
        service.request(paths[3].clone(), 2, ThumbnailPriority::Visible),
        RequestStatus::QueueFull
    );
    assert_eq!(
        service.request(
            directory.path().join("overflow.png"),
            2,
            ThumbnailPriority::Prefetch
        ),
        RequestStatus::QueueFull
    );
    assert_eq!(service.queued_len(), 1);

    gate.release();
    let mut responses = Vec::new();
    while responses.len() < 3 {
        responses
            .push(response_for(&service, 2, Duration::from_secs(3)).expect("prioritized response"));
    }
    assert!(responses.iter().any(|response| response.path == paths[2]));
    assert!(responses.iter().all(|response| response.path != paths[3]));
}

#[test]
fn retain_cancels_paths_that_left_the_viewport() {
    let directory = tempdir().expect("temporary directory");
    let visible = directory.path().join("visible.png");
    let invisible = directory.path().join("invisible.png");
    write_png(&visible);
    write_png(&invisible);
    let gate = LoadGate::new();
    let service = service(&gate, 8);

    assert_eq!(
        service.request(visible.clone(), 4, ThumbnailPriority::Visible),
        RequestStatus::Queued
    );
    assert_eq!(
        service.request(invisible.clone(), 4, ThumbnailPriority::Prefetch),
        RequestStatus::Queued
    );
    assert!(gate.wait_until_entered(2, Duration::from_secs(2)));
    assert_eq!(service.retain(4, &HashSet::from([visible.clone()])), 1);
    gate.release();

    let response = response_for(&service, 4, Duration::from_secs(2)).expect("visible response");
    assert_eq!(response.path, visible);
    wait_idle(&service);
    assert!(service.poll().is_none());
}

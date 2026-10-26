use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use liteview::{
    tile::TileCoord,
    tile_service::{TileDecodeRequest, TileDecoder, TileRequestKey, TileService},
};

struct FakeDecoder {
    calls: Arc<AtomicUsize>,
    release: Option<Mutex<mpsc::Receiver<()>>>,
}

impl TileDecoder for FakeDecoder {
    fn decode(&self, request: &TileDecodeRequest) -> Result<Vec<u8>, String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if let Some(release) = &self.release {
            let _ = release
                .lock()
                .expect("release lock")
                .recv_timeout(Duration::from_secs(2));
        }
        let pixels = request.output_size[0] as usize * request.output_size[1] as usize;
        Ok(vec![127; pixels * 4])
    }
}

fn key(generation: u64, name: &str, column: u32) -> TileRequestKey {
    TileRequestKey {
        generation,
        path: PathBuf::from(name),
        coord: TileCoord {
            level: 0,
            column,
            row: 0,
        },
    }
}

fn request(name: &str) -> TileDecodeRequest {
    TileDecodeRequest {
        path: PathBuf::from(name),
        source_dimensions: [16, 16],
        source_rect: [0, 0, 16, 16],
        output_size: [16, 16],
        exif_orientation: 1,
        icc_profile: None,
    }
}

#[test]
fn deduplicates_pending_tiles_and_returns_pixels() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut service = TileService::new_with_worker_count(
        Arc::new(FakeDecoder {
            calls: Arc::clone(&calls),
            release: None,
        }),
        1,
    )
    .expect("start service");
    let tile = key(1, "a.jpg", 0);

    assert!(service.request(request("a.jpg"), tile.clone()));
    assert!(service.request(request("a.jpg"), tile.clone()));

    let response = (0..100)
        .find_map(|_| {
            if let Some(response) = service.poll() {
                Some(response)
            } else {
                thread::sleep(Duration::from_millis(5));
                None
            }
        })
        .expect("tile response");
    assert_eq!(response.key, tile);
    assert_eq!(response.result.expect("tile pixels").len(), 16 * 16 * 4);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn cancels_old_generations_without_emitting_stale_pixels() {
    let (release_tx, release_rx) = mpsc::channel();
    let mut service = TileService::new_with_worker_count(
        Arc::new(FakeDecoder {
            calls: Arc::new(AtomicUsize::new(0)),
            release: Some(Mutex::new(release_rx)),
        }),
        1,
    )
    .expect("start service");
    let old = key(1, "old.jpg", 0);

    assert!(service.request(request("old.jpg"), old));
    service.cancel_before(2);
    let _ = release_tx.send(());

    thread::sleep(Duration::from_millis(50));
    assert!(service.poll().is_none());
    assert_eq!(service.pending_count(), 0);
}

#[test]
fn rejects_work_when_the_bounded_queue_is_full() {
    let (release_tx, release_rx) = mpsc::channel();
    let mut service = TileService::new_with_options(
        Arc::new(FakeDecoder {
            calls: Arc::new(AtomicUsize::new(0)),
            release: Some(Mutex::new(release_rx)),
        }),
        1,
        1,
    )
    .expect("start service");

    assert!(service.request(request("first.jpg"), key(1, "first.jpg", 0)));
    thread::sleep(Duration::from_millis(20));
    assert!(service.request(request("second.jpg"), key(1, "second.jpg", 0)));
    assert!(!service.request(request("third.jpg"), key(1, "third.jpg", 0)));
    let _ = release_tx.send(());
}

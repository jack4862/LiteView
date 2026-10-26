use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use crate::loader::{self, LoadOptions, LoadedImage};

struct LoadRequest {
    id: u64,
    generation: u64,
    path: PathBuf,
    options: LoadOptions,
    cancel: Arc<AtomicBool>,
}

pub(super) struct LoadResponse {
    id: u64,
    pub(super) generation: u64,
    pub(super) path: PathBuf,
    pub(super) result: Result<LoadedImage, String>,
}

struct PendingRequest {
    path: PathBuf,
    cancel: Arc<AtomicBool>,
}

pub(super) struct LoadService {
    requests: Sender<LoadRequest>,
    responses: Receiver<LoadResponse>,
    pending: HashMap<u64, PendingRequest>,
    next_id: u64,
}

impl LoadService {
    pub(super) fn new() -> Result<Self, String> {
        let (request_tx, request_rx) = mpsc::channel::<LoadRequest>();
        let (response_tx, response_rx) = mpsc::channel::<LoadResponse>();
        let request_rx = Arc::new(Mutex::new(request_rx));
        let cpu_count = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(2);
        let worker_count = cpu_count.saturating_sub(1).clamp(1, 4);
        let mut started = 0;

        for index in 0..worker_count {
            let requests = Arc::clone(&request_rx);
            let responses = response_tx.clone();
            let result = thread::Builder::new()
                .name(format!("liteview-loader-{index}"))
                .spawn(move || {
                    loop {
                        let request = {
                            let Ok(receiver) = requests.lock() else {
                                break;
                            };
                            let Ok(request) = receiver.recv() else {
                                break;
                            };
                            request
                        };
                        if request.cancel.load(Ordering::Relaxed) {
                            continue;
                        }
                        let result = loader::load(&request.path, &request.options)
                            .map_err(|error| error.to_string());
                        if request.cancel.load(Ordering::Relaxed) {
                            continue;
                        }
                        let _ = responses.send(LoadResponse {
                            id: request.id,
                            generation: request.generation,
                            path: request.path,
                            result,
                        });
                    }
                });
            if result.is_ok() {
                started += 1;
            }
        }

        if started == 0 {
            return Err("could not start any image loader threads".to_owned());
        }
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
            pending: HashMap::new(),
            next_id: 0,
        })
    }

    pub(super) fn request(&mut self, path: PathBuf, generation: u64, options: LoadOptions) -> bool {
        if self.is_pending(&path) {
            return true;
        }
        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        let cancel = Arc::new(AtomicBool::new(false));
        let pending_path = path.clone();
        let sent = self
            .requests
            .send(LoadRequest {
                id,
                generation,
                path,
                options,
                cancel: Arc::clone(&cancel),
            })
            .is_ok();
        if sent {
            self.pending.insert(
                id,
                PendingRequest {
                    path: pending_path,
                    cancel,
                },
            );
        }
        sent
    }

    pub(super) fn cancel_all(&mut self) {
        for pending in self.pending.drain().map(|(_, pending)| pending) {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn is_pending(&self, path: &Path) -> bool {
        self.pending.values().any(|pending| pending.path == path)
    }

    pub(super) fn is_busy(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(super) fn poll(&mut self) -> Option<LoadResponse> {
        let response = self.responses.try_recv().ok()?;
        self.pending.remove(&response.id);
        Some(response)
    }
}

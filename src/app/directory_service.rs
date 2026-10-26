use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
};

use crate::navigation::{DirectorySnapshot, NavigationError};

pub(super) struct DirectoryResponse {
    pub(super) generation: u64,
    pub(super) selected: PathBuf,
    pub(super) result: Result<DirectorySnapshot, NavigationError>,
}

struct DirectoryRequest {
    generation: u64,
    selected: PathBuf,
}

pub(super) struct DirectoryService {
    requests: SyncSender<DirectoryRequest>,
    responses: Receiver<DirectoryResponse>,
}

impl DirectoryService {
    pub(super) fn new() -> Result<Self, String> {
        let (request_tx, request_rx) = mpsc::sync_channel::<DirectoryRequest>(2);
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("liteview-directory".to_owned())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let result = DirectorySnapshot::scan(&request.selected);
                    if response_tx
                        .send(DirectoryResponse {
                            generation: request.generation,
                            selected: request.selected,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            requests: request_tx,
            responses: response_rx,
        })
    }

    pub(super) fn request(&self, selected: PathBuf, generation: u64) -> bool {
        self.requests
            .try_send(DirectoryRequest {
                generation,
                selected,
            })
            .is_ok()
    }

    pub(super) fn poll(&self) -> Option<DirectoryResponse> {
        self.responses.try_recv().ok()
    }
}

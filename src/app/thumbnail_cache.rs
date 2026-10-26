use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use eframe::egui;

pub(super) struct CachedThumbnail {
    pub(super) texture: egui::TextureHandle,
    pub(super) bytes: usize,
}

pub(super) struct ThumbnailCache {
    entries: HashMap<PathBuf, CachedThumbnail>,
    last_used: HashMap<PathBuf, u64>,
    total_bytes: usize,
    max_bytes: usize,
    clock: u64,
}

impl ThumbnailCache {
    pub(super) fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            last_used: HashMap::new(),
            total_bytes: 0,
            max_bytes,
            clock: 0,
        }
    }

    pub(super) fn get(&self, path: &Path) -> Option<&CachedThumbnail> {
        self.entries.get(path)
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.entries.contains_key(path)
    }

    pub(super) fn mark_used(&mut self, path: &Path) {
        self.clock = self.clock.wrapping_add(1);
        self.last_used.insert(path.to_path_buf(), self.clock);
    }

    pub(super) fn insert(&mut self, path: PathBuf, thumbnail: CachedThumbnail) {
        self.clock = self.clock.wrapping_add(1);
        self.last_used.insert(path.clone(), self.clock);
        if let Some(previous) = self.entries.remove(&path) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(thumbnail.bytes);
        self.entries.insert(path, thumbnail);
        self.evict_to_limit();
    }

    pub(super) fn retain_directory(&mut self, directory: &Path) {
        let before = self.entries.len();
        self.entries
            .retain(|path, _| path.parent() == Some(directory));
        if self.entries.len() != before {
            self.last_used
                .retain(|path, _| self.entries.contains_key(path));
            self.total_bytes = self.entries.values().map(|thumbnail| thumbnail.bytes).sum();
        }
    }

    pub(super) fn set_max_bytes(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes.max(1);
        self.evict_to_limit();
    }

    fn evict_to_limit(&mut self) {
        while self.total_bytes > self.max_bytes {
            let Some(path) = self
                .entries
                .keys()
                .min_by_key(|path| self.last_used.get(*path).copied().unwrap_or(0))
                .cloned()
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&path) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.bytes);
            }
            self.last_used.remove(&path);
        }
    }
}

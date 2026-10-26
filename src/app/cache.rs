use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use eframe::egui;

use crate::loader::LoadedImage;

pub(super) struct CachedPreview {
    pub(super) image: LoadedImage,
    pub(super) tile_icc_profile: Option<Arc<[u8]>>,
    pub(super) texture: egui::TextureHandle,
    pub(super) bytes: usize,
}

pub(super) struct PreviewCache {
    entries: HashMap<PathBuf, CachedPreview>,
    total_bytes: usize,
    max_bytes: usize,
    max_entries: usize,
    clock: u64,
    last_used: HashMap<PathBuf, u64>,
}

impl PreviewCache {
    pub(super) fn new(max_bytes: usize, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            total_bytes: 0,
            max_bytes,
            max_entries,
            clock: 0,
            last_used: HashMap::new(),
        }
    }

    pub(super) fn get(&self, path: &Path) -> Option<&CachedPreview> {
        self.entries.get(path)
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.entries.contains_key(path)
    }

    pub(super) fn mark_used(&mut self, path: &Path) {
        self.clock = self.clock.wrapping_add(1);
        self.last_used.insert(path.to_path_buf(), self.clock);
    }

    pub(super) fn insert(&mut self, path: PathBuf, preview: CachedPreview, current: Option<&Path>) {
        self.clock = self.clock.wrapping_add(1);
        self.last_used.insert(path.clone(), self.clock);
        if let Some(old) = self.entries.remove(&path) {
            self.total_bytes = self.total_bytes.saturating_sub(old.bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(preview.bytes);
        self.entries.insert(path, preview);
        self.evict_to_limits(current);
    }

    pub(super) fn set_limits(
        &mut self,
        max_bytes: usize,
        max_entries: usize,
        current: Option<&Path>,
    ) {
        self.max_bytes = max_bytes.max(1);
        self.max_entries = max_entries.max(1);
        self.evict_to_limits(current);
    }

    fn evict_to_limits(&mut self, current: Option<&Path>) {
        while self.total_bytes > self.max_bytes || self.entries.len() > self.max_entries {
            let candidate = self
                .entries
                .keys()
                .filter(|path| current != Some(path.as_path()))
                .min_by_key(|path| self.last_used.get(*path).copied().unwrap_or(0))
                .cloned();
            let Some(path) = candidate else {
                break;
            };
            if let Some(removed) = self.entries.remove(&path) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.bytes);
            }
            self.last_used.remove(&path);
        }
    }
}

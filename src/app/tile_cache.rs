use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use eframe::egui;

use crate::tile::TileCoord;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct TileCacheKey {
    pub(super) path: PathBuf,
    pub(super) coord: TileCoord,
}

pub(super) struct CachedTile {
    pub(super) texture: egui::TextureHandle,
    pub(super) source_rect: [u32; 4],
    pub(super) bytes: usize,
}

pub(super) struct TileCache {
    entries: HashMap<TileCacheKey, CachedTile>,
    last_used: HashMap<TileCacheKey, u64>,
    protected: HashSet<TileCacheKey>,
    total_bytes: usize,
    max_bytes: usize,
    clock: u64,
}

impl TileCache {
    pub(super) fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            last_used: HashMap::new(),
            protected: HashSet::new(),
            total_bytes: 0,
            max_bytes,
            clock: 0,
        }
    }

    pub(super) fn get(&self, key: &TileCacheKey) -> Option<&CachedTile> {
        self.entries.get(key)
    }

    pub(super) fn mark_used(&mut self, key: &TileCacheKey) {
        self.clock = self.clock.wrapping_add(1);
        self.last_used.insert(key.clone(), self.clock);
    }

    pub(super) fn insert(&mut self, key: TileCacheKey, tile: CachedTile) {
        self.clock = self.clock.wrapping_add(1);
        self.last_used.insert(key.clone(), self.clock);
        if let Some(previous) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(tile.bytes);
        self.entries.insert(key, tile);
        self.evict_to_limit();
    }

    pub(super) fn set_protected(&mut self, path: &Path, visible: &HashSet<TileCoord>) {
        self.protected.clear();
        self.protected
            .extend(visible.iter().map(|coord| TileCacheKey {
                path: path.to_path_buf(),
                coord: *coord,
            }));
        self.evict_to_limit();
    }

    pub(super) fn set_max_bytes(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes.max(1);
        self.evict_to_limit();
    }

    pub(super) fn remove_path(&mut self, path: &Path) {
        let keys: Vec<_> = self
            .entries
            .keys()
            .filter(|key| key.path == path)
            .cloned()
            .collect();
        for key in keys {
            if let Some(removed) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.bytes);
            }
            self.last_used.remove(&key);
        }
    }

    fn evict_to_limit(&mut self) {
        while self.total_bytes > self.max_bytes {
            let Some(key) = self
                .entries
                .keys()
                .filter(|key| !self.protected.contains(*key))
                .min_by_key(|key| self.last_used.get(*key).copied().unwrap_or(0))
                .cloned()
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.bytes);
            }
            self.last_used.remove(&key);
        }
    }
}

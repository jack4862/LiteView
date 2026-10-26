use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct SessionState {
    generation: u64,
    current: Option<PathBuf>,
}

impl SessionState {
    pub fn begin(&mut self, path: PathBuf) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.current = Some(path);
        self.generation
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_current_generation(&self, generation: u64) -> bool {
        generation == self.generation
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.current.as_deref()
    }

    pub fn accepts(&self, generation: u64, path: &Path) -> bool {
        self.is_current_generation(generation) && self.current.as_deref() == Some(path)
    }
}

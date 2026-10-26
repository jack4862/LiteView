use std::{
    cmp::Ordering,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

#[derive(Debug, Error)]
pub enum NavigationError {
    #[error("cannot scan {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the selected image has an unsupported extension")]
    SelectedUnsupported,
    #[error("the selected image was not found in its directory")]
    SelectedNotFound,
}

#[derive(Debug, Clone)]
pub struct DirectorySnapshot {
    directory: PathBuf,
    files: Vec<PathBuf>,
    selected_index: usize,
}

impl DirectorySnapshot {
    pub fn scan(selected: impl AsRef<Path>) -> Result<Self, NavigationError> {
        let selected = selected.as_ref();
        if !is_supported_path(selected) {
            return Err(NavigationError::SelectedUnsupported);
        }

        let directory = selected.parent().unwrap_or_else(|| Path::new("."));
        let entries = fs::read_dir(directory).map_err(|source| NavigationError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let mut files = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|source| NavigationError::Io {
                path: directory.to_path_buf(),
                source,
            })?;
            let file_type = entry.file_type().map_err(|source| NavigationError::Io {
                path: entry.path(),
                source,
            })?;
            if file_type.is_file() && is_supported_path(&entry.path()) {
                files.push(entry.path());
            }
        }

        files.sort_by(|left, right| natural_path_cmp(left, right));
        let selected_name = selected.file_name();
        let selected_index = files
            .iter()
            .position(|path| path.file_name() == selected_name)
            .ok_or(NavigationError::SelectedNotFound)?;

        Ok(Self {
            directory: directory.to_path_buf(),
            files,
            selected_index,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn current(&self) -> Option<&Path> {
        self.files.get(self.selected_index).map(PathBuf::as_path)
    }

    pub fn previous(&self) -> Option<&Path> {
        self.selected_index
            .checked_sub(1)
            .and_then(|index| self.files.get(index))
            .map(PathBuf::as_path)
    }

    pub fn next(&self) -> Option<&Path> {
        self.files
            .get(self.selected_index + 1)
            .map(PathBuf::as_path)
    }

    pub fn set_selected_index(&mut self, index: usize) -> bool {
        if index >= self.files.len() {
            return false;
        }
        self.selected_index = index;
        true
    }
}

pub fn is_supported_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

pub fn natural_path_cmp(left: &Path, right: &Path) -> Ordering {
    let left_name = left.file_name().unwrap_or(left.as_os_str());
    let right_name = right.file_name().unwrap_or(right.as_os_str());
    natural_os_str_cmp(left_name, right_name)
}

fn natural_os_str_cmp(left: &OsStr, right: &OsStr) -> Ordering {
    let left_text = left.to_string_lossy();
    let right_text = right.to_string_lossy();
    natural_text_cmp(&left_text, &right_text).then_with(|| left.cmp(right))
}

fn natural_text_cmp(left: &str, right: &str) -> Ordering {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut left_index = 0;
    let mut right_index = 0;

    while left_index < left.len() && right_index < right.len() {
        if left[left_index].is_ascii_digit() && right[right_index].is_ascii_digit() {
            let ordering = compare_number(&left, &mut left_index, &right, &mut right_index);
            if ordering != Ordering::Equal {
                return ordering;
            }
            continue;
        }

        let left_char = left[left_index].to_ascii_lowercase();
        let right_char = right[right_index].to_ascii_lowercase();
        if left_char != right_char {
            return left_char.cmp(&right_char);
        }
        left_index += 1;
        right_index += 1;
    }

    match (left_index == left.len(), right_index == right.len()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => left.len().cmp(&right.len()),
    }
}

fn compare_number(
    left: &[char],
    left_index: &mut usize,
    right: &[char],
    right_index: &mut usize,
) -> Ordering {
    while left.get(*left_index) == Some(&'0') {
        *left_index += 1;
    }
    while right.get(*right_index) == Some(&'0') {
        *right_index += 1;
    }

    let left_start = *left_index;
    let right_start = *right_index;
    while left.get(*left_index).is_some_and(char::is_ascii_digit) {
        *left_index += 1;
    }
    while right.get(*right_index).is_some_and(char::is_ascii_digit) {
        *right_index += 1;
    }

    left[left_start..*left_index]
        .len()
        .cmp(&right[right_start..*right_index].len())
        .then_with(|| left[left_start..*left_index].cmp(&right[right_start..*right_index]))
}

#[cfg(test)]
mod tests {
    use super::natural_text_cmp;
    use std::cmp::Ordering;

    #[test]
    fn compares_numeric_chunks_by_value() {
        assert_eq!(
            natural_text_cmp("photo2.png", "photo10.png"),
            Ordering::Less
        );
        assert_eq!(
            natural_text_cmp("photo02.png", "photo2.png"),
            Ordering::Equal
        );
        assert_eq!(
            natural_text_cmp("Photo3.png", "photo3.png"),
            Ordering::Equal
        );
    }
}

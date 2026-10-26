use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use liteview::config::{
    AppSettings, DirectoryView, Language, MAX_CACHE_LIMIT_MIB, MAX_MANUAL_ZOOM,
    MIN_CACHE_LIMIT_MIB, MIN_MANUAL_ZOOM, SettingsStore, ZoomMode,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let base = env::temp_dir().join("liteview-config-contracts");
        fs::create_dir_all(&base).expect("create test base directory");

        for _ in 0..32 {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }

        panic!("could not allocate a unique test directory");
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn saves_restores_and_replaces_settings_atomically() {
    let directory = TestDirectory::new();
    let settings_directory = directory.path().join("nested");
    let store = SettingsStore::new(settings_directory.join("settings.ini"));
    let expected = AppSettings {
        language: Language::Chinese,
        zoom_mode: ZoomMode::Manual,
        manual_zoom: 2.75,
        show_metadata: true,
        directory_open: true,
        directory_view: DirectoryView::List,
        cache_limit_mib: 512,
    };

    store.save(&expected).expect("save initial settings");
    assert_eq!(store.load(), expected);

    let replacement = AppSettings {
        language: Language::English,
        zoom_mode: ZoomMode::Fit,
        manual_zoom: 1.5,
        show_metadata: false,
        directory_open: false,
        directory_view: DirectoryView::Grid,
        cache_limit_mib: 128,
    };
    store.save(&replacement).expect("replace existing settings");

    assert_eq!(store.load(), replacement);
    let files: Vec<_> = fs::read_dir(&settings_directory)
        .expect("read test directory")
        .map(|entry| entry.expect("directory entry").file_name())
        .collect();
    assert_eq!(files, [settings_file_name()]);
}

#[test]
fn missing_and_corrupt_files_fall_back_safely() {
    let directory = TestDirectory::new();
    let store = SettingsStore::new(directory.path().join("settings.ini"));

    assert_eq!(store.load(), AppSettings::default());

    fs::write(store.path(), b"not an ini file\n= broken\n[unterminated").expect("write corruption");
    assert_eq!(store.load(), AppSettings::default());
}

#[test]
fn invalid_values_are_clamped_and_invalid_fields_keep_defaults() {
    let directory = TestDirectory::new();
    let store = SettingsStore::new(directory.path().join("settings.ini"));
    fs::write(
        store.path(),
        b"[settings]\n\
          zoom_mode=manual\n\
          manual_zoom=1000\n\
          show_metadata=not-a-boolean\n\
          directory_open=not-a-boolean\n\
          directory_view=carousel\n\
          cache_limit_mib=-100\n",
    )
    .expect("write out-of-range settings");

    let loaded = store.load();
    assert_eq!(loaded.zoom_mode, ZoomMode::Manual);
    assert_eq!(loaded.manual_zoom, MAX_MANUAL_ZOOM);
    assert_eq!(loaded.show_metadata, AppSettings::default().show_metadata);
    assert_eq!(loaded.directory_open, AppSettings::default().directory_open);
    assert_eq!(loaded.directory_view, AppSettings::default().directory_view);
    assert_eq!(loaded.cache_limit_mib, MIN_CACHE_LIMIT_MIB);

    let mut saved_out_of_range = AppSettings {
        language: Language::System,
        zoom_mode: ZoomMode::Manual,
        manual_zoom: -3.0,
        show_metadata: true,
        directory_open: true,
        directory_view: DirectoryView::List,
        cache_limit_mib: u32::MAX,
    };
    store
        .save(&saved_out_of_range)
        .expect("save out-of-range settings");
    let restored = store.load();
    assert_eq!(restored.manual_zoom, MIN_MANUAL_ZOOM);
    assert_eq!(restored.cache_limit_mib, MAX_CACHE_LIMIT_MIB);

    saved_out_of_range.normalize();
    assert_eq!(saved_out_of_range, restored);
}

#[test]
fn default_path_uses_a_safe_liteview_location() {
    let path = SettingsStore::default_path();
    let expected_suffix = Path::new("LiteView").join("settings.ini");

    assert!(path.ends_with(expected_suffix));

    #[cfg(windows)]
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        assert_eq!(
            path,
            PathBuf::from(local_app_data)
                .join("LiteView")
                .join("settings.ini")
        );
    }
}

#[test]
fn zoom_and_directory_modes_serialize_to_stable_names() {
    assert_eq!(ZoomMode::Fit.to_string(), "fit");
    assert_eq!(ZoomMode::Manual.to_string(), "manual");
    assert_eq!("FIT".parse::<ZoomMode>(), Ok(ZoomMode::Fit));
    assert!("actual".parse::<ZoomMode>().is_err());

    assert_eq!(DirectoryView::Grid.to_string(), "grid");
    assert_eq!(DirectoryView::List.to_string(), "list");
    assert_eq!("LIST".parse::<DirectoryView>(), Ok(DirectoryView::List));
    assert!("details".parse::<DirectoryView>().is_err());
}

fn settings_file_name() -> std::ffi::OsString {
    std::ffi::OsString::from("settings.ini")
}

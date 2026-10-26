use std::{
    env, fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MIN_MANUAL_ZOOM: f32 = 0.01;
pub const MAX_MANUAL_ZOOM: f32 = 32.0;
pub const MIN_CACHE_LIMIT_MIB: u32 = 32;
pub const MAX_CACHE_LIMIT_MIB: u32 = 2_048;
pub const DEFAULT_CACHE_LIMIT_MIB: u32 = 256;

const DEFAULT_MANUAL_ZOOM: f32 = 1.0;
const SETTINGS_SECTION: &str = "settings";
const SETTINGS_FILE_NAME: &str = "settings.ini";
const APP_DIRECTORY_NAME: &str = "LiteView";
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    Fit,
    Manual,
}

impl ZoomMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Manual => "manual",
        }
    }
}

impl fmt::Display for ZoomMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ZoomMode {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fit" => Ok(Self::Fit),
            "manual" => Ok(Self::Manual),
            _ => Err(ParseEnumError {
                expected: "fit or manual",
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirectoryView {
    #[default]
    Grid,
    List,
}

impl DirectoryView {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::List => "list",
        }
    }
}

impl fmt::Display for DirectoryView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DirectoryView {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "grid" => Ok(Self::Grid),
            "list" => Ok(Self::List),
            _ => Err(ParseEnumError {
                expected: "grid or list",
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseEnumError {
    expected: &'static str,
}

impl fmt::Display for ParseEnumError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "expected {}", self.expected)
    }
}

impl std::error::Error for ParseEnumError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiLanguage {
    English,
    Chinese,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    System,
    English,
    Chinese,
}

impl Language {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "english",
            Self::Chinese => "chinese",
        }
    }

    pub fn resolve(self, locale: Option<&str>) -> UiLanguage {
        match self {
            Self::English => UiLanguage::English,
            Self::Chinese => UiLanguage::Chinese,
            Self::System => {
                let language = locale
                    .map(|locale| locale.split(['-', '_']).next().unwrap_or(""))
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if language == "zh" {
                    UiLanguage::Chinese
                } else {
                    UiLanguage::English
                }
            }
        }
    }

    pub fn system_ui_language() -> UiLanguage {
        Self::System.resolve(sys_locale::get_locale().as_deref())
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Language {
    type Err = ParseLanguageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => Ok(Self::System),
            "english" | "en" => Ok(Self::English),
            "chinese" | "zh" | "zh-hans" => Ok(Self::Chinese),
            _ => Err(ParseLanguageError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLanguageError;

impl fmt::Display for ParseLanguageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected system, english, or chinese")
    }
}

impl std::error::Error for ParseLanguageError {}

#[derive(Debug, Clone, PartialEq)]
pub struct AppSettings {
    pub language: Language,
    pub zoom_mode: ZoomMode,
    pub manual_zoom: f32,
    pub show_metadata: bool,
    pub directory_open: bool,
    pub directory_view: DirectoryView,
    pub cache_limit_mib: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            zoom_mode: ZoomMode::Fit,
            manual_zoom: DEFAULT_MANUAL_ZOOM,
            show_metadata: false,
            directory_open: false,
            directory_view: DirectoryView::Grid,
            cache_limit_mib: DEFAULT_CACHE_LIMIT_MIB,
        }
    }
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.manual_zoom = normalize_manual_zoom(self.manual_zoom);
        self.cache_limit_mib = self
            .cache_limit_mib
            .clamp(MIN_CACHE_LIMIT_MIB, MAX_CACHE_LIMIT_MIB);
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    pub fn from_ini(contents: &str) -> Self {
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
        let mut in_settings = false;
        let mut settings = Self::default();

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                let section = line[1..line.len() - 1].trim();
                in_settings = section.eq_ignore_ascii_case(SETTINGS_SECTION);
                continue;
            }
            if !in_settings {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            apply_setting(&mut settings, key.trim(), value.trim());
        }

        settings.normalized()
    }

    pub fn to_ini(&self) -> String {
        let settings = self.clone().normalized();
        format!(
            "[{SETTINGS_SECTION}]\n\
             language={}\n\
             zoom_mode={}\n\
             manual_zoom={}\n\
             show_metadata={}\n\
             directory_open={}\n\
             directory_view={}\n\
             cache_limit_mib={}\n",
            settings.language,
            settings.zoom_mode,
            settings.manual_zoom,
            settings.show_metadata,
            settings.directory_open,
            settings.directory_view,
            settings.cache_limit_mib
        )
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        user_config_directory()
            .join(APP_DIRECTORY_NAME)
            .join(SETTINGS_FILE_NAME)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> AppSettings {
        fs::read_to_string(&self.path)
            .map(|contents| AppSettings::from_ini(&contents))
            .unwrap_or_default()
    }

    pub fn save(&self, settings: &AppSettings) -> io::Result<()> {
        let contents = settings.to_ini();
        write_atomically(&self.path, contents.as_bytes())
    }
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new(Self::default_path())
    }
}

fn apply_setting(settings: &mut AppSettings, key: &str, value: &str) {
    match key.to_ascii_lowercase().as_str() {
        "language" => {
            if let Ok(language) = value.parse() {
                settings.language = language;
            }
        }
        "zoom_mode" => {
            if let Ok(mode) = value.parse() {
                settings.zoom_mode = mode;
            }
        }
        "manual_zoom" => {
            if let Some(zoom) = parse_manual_zoom(value) {
                settings.manual_zoom = zoom;
            }
        }
        "show_metadata" => {
            if let Some(show_metadata) = parse_bool(value) {
                settings.show_metadata = show_metadata;
            }
        }
        "directory_open" => {
            if let Some(directory_open) = parse_bool(value) {
                settings.directory_open = directory_open;
            }
        }
        "directory_view" => {
            if let Ok(view) = value.parse() {
                settings.directory_view = view;
            }
        }
        "cache_limit_mib" => {
            if let Some(cache_limit_mib) = parse_cache_limit(value) {
                settings.cache_limit_mib = cache_limit_mib;
            }
        }
        _ => {}
    }
}

fn parse_manual_zoom(value: &str) -> Option<f32> {
    let zoom = value.parse::<f64>().ok()?;
    if !zoom.is_finite() {
        return None;
    }

    Some(zoom.clamp(f64::from(MIN_MANUAL_ZOOM), f64::from(MAX_MANUAL_ZOOM)) as f32)
}

fn normalize_manual_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_MANUAL_ZOOM, MAX_MANUAL_ZOOM)
    } else {
        DEFAULT_MANUAL_ZOOM
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_cache_limit(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Ok(value) = value.parse::<i64>() {
        return Some(value.clamp(
            i64::from(MIN_CACHE_LIMIT_MIB),
            i64::from(MAX_CACHE_LIMIT_MIB),
        ) as u32);
    }
    if let Ok(value) = value.parse::<u64>() {
        return Some(value.min(u64::from(MAX_CACHE_LIMIT_MIB)) as u32);
    }

    None
}

fn user_config_directory() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(path) = non_empty_env_path("LOCALAPPDATA") {
            return path;
        }
        if let Some(path) = non_empty_env_path("USERPROFILE") {
            return path.join("AppData").join("Local");
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(path) = non_empty_env_path("XDG_CONFIG_HOME") {
            return path;
        }
        if let Some(path) = non_empty_env_path("HOME") {
            return path.join(".config");
        }
    }

    env::temp_dir()
}

fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = usable_parent(path);
    fs::create_dir_all(parent)?;
    let (temporary_path, mut file) = create_temporary_file(path, parent)?;

    let write_result = (|| {
        file.write_all(contents)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary_path, path)
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

fn create_temporary_file(path: &Path, parent: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = path
        .file_name()
        .unwrap_or_else(|| SETTINGS_FILE_NAME.as_ref())
        .to_string_lossy();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for _ in 0..32 {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{file_name}.{}.{timestamp}.{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique settings temporary file",
    ))
}

fn usable_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

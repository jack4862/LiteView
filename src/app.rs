mod cache;
mod directory_service;
mod loader_service;
mod painting;
mod thumbnail_cache;
mod tile_cache;

use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use eframe::egui::{
    self, Align, Color32, Label, Layout, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind,
    TextureOptions, Vec2, ViewportCommand,
};

use self::{
    cache::{CachedPreview, PreviewCache},
    directory_service::DirectoryService,
    loader_service::LoadService,
    painting::{
        TileGeometry, draw_checkerboard, draw_drop_overlay, draw_empty_state, draw_loading_pill,
        draw_textured_image, draw_textured_tile,
    },
    thumbnail_cache::{CachedThumbnail, ThumbnailCache},
    tile_cache::{CachedTile, TileCache, TileCacheKey},
};
use crate::{
    config::{
        AppSettings, DirectoryView, Language, MAX_CACHE_LIMIT_MIB, MIN_CACHE_LIMIT_MIB,
        SettingsStore, UiLanguage, ZoomMode,
    },
    i18n::texts,
    loader::{IccStatus, LoadOptions, LoadedImage, SourceFormat, capped_dimensions},
    navigation::DirectorySnapshot,
    session::SessionState,
    thumbnail_service::{ThumbnailConfig, ThumbnailPriority, ThumbnailService},
    tile::{TileGrid, level_for_zoom, visible_source_rect},
    tile_decoder,
    tile_service::{TileDecodeRequest, TileRequestKey, TileService},
    view::ViewState,
};

const TOOLBAR_HEIGHT: f32 = 44.0;
const STATUS_HEIGHT: f32 = 28.0;
const CACHE_MAX_ENTRIES: usize = 6;
const PRELOAD_SIDE: u32 = 2048;
const THUMBNAIL_SIDE: u32 = 320;
const THUMBNAIL_CARD_WIDTH: f32 = 172.0;
const THUMBNAIL_CARD_HEIGHT: f32 = 178.0;
const THUMBNAIL_ROW_HEIGHT: f32 = 188.0;
const MIB: usize = 1024 * 1024;
const MAX_THUMBNAIL_UPLOAD_PER_FRAME: usize = 8 * MIB;
const MAX_TILE_UPLOAD_PER_FRAME: usize = 8 * MIB;
const VIEWER_BG: Color32 = Color32::from_rgb(24, 26, 29);
const PANEL_BG: Color32 = Color32::from_rgb(34, 37, 41);
const STATUS_BG: Color32 = Color32::from_rgb(29, 31, 35);
const MUTED: Color32 = Color32::from_rgb(158, 164, 172);
const WARNING: Color32 = Color32::from_rgb(236, 184, 92);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheBudget {
    preview: usize,
    tiles: usize,
    thumbnails: usize,
}

impl CacheBudget {
    fn for_limit(limit_mib: u32) -> Self {
        let total = limit_mib as usize * MIB;
        let preview = total / 4;
        let tiles = total.saturating_mul(5) / 8;
        let thumbnails = total.saturating_sub(preview).saturating_sub(tiles);
        Self {
            preview,
            tiles,
            thumbnails,
        }
    }
}

pub struct LiteViewApp {
    loader: LoadService,
    directory_service: DirectoryService,
    thumbnail_service: ThumbnailService,
    tile_service: TileService,
    session: SessionState,
    snapshot: Option<DirectorySnapshot>,
    cache: PreviewCache,
    thumbnail_cache: ThumbnailCache,
    tile_cache: TileCache,
    view: ViewState,
    settings: AppSettings,
    settings_store: SettingsStore,
    settings_draft: Option<AppSettings>,
    ui_language: UiLanguage,
    metadata_visible: bool,
    fullscreen: bool,
    loading_current: bool,
    error: Option<String>,
    warning: Option<String>,
    directory_error: Option<String>,
    thumbnail_failures: HashSet<PathBuf>,
    tile_disabled: bool,
    directory_generation: u64,
    tile_generation: u64,
    texture_serial: u64,
    last_viewport: Rect,
    window_title: String,
}

impl LiteViewApp {
    pub fn new(
        creation: &eframe::CreationContext<'_>,
        initial_path: Option<PathBuf>,
    ) -> io::Result<Self> {
        configure_style(&creation.egui_ctx);
        let settings_store = SettingsStore::default();
        let settings = settings_store.load();
        let budget = CacheBudget::for_limit(settings.cache_limit_mib);
        let locale = sys_locale::get_locale();
        let ui_language = settings.language.resolve(locale.as_deref());
        let thumbnail_config = ThumbnailConfig {
            worker_count: 2,
            queue_capacity: 48,
            max_preview_side: THUMBNAIL_SIDE,
            ..ThumbnailConfig::default()
        };
        let mut app = Self {
            loader: LoadService::new().map_err(io::Error::other)?,
            directory_service: DirectoryService::new().map_err(io::Error::other)?,
            thumbnail_service: ThumbnailService::new(thumbnail_config).map_err(io::Error::other)?,
            tile_service: TileService::new_with_worker_count(tile_decoder::platform_decoder(), 2)
                .map_err(io::Error::other)?,
            session: SessionState::default(),
            snapshot: None,
            cache: PreviewCache::new(budget.preview, CACHE_MAX_ENTRIES),
            thumbnail_cache: ThumbnailCache::new(budget.thumbnails),
            tile_cache: TileCache::new(budget.tiles),
            view: ViewState::with_mode(settings.zoom_mode == ZoomMode::Fit, settings.manual_zoom),
            metadata_visible: settings.show_metadata,
            settings,
            settings_store,
            settings_draft: None,
            ui_language,
            fullscreen: false,
            loading_current: false,
            error: None,
            warning: None,
            directory_error: None,
            thumbnail_failures: HashSet::new(),
            tile_disabled: false,
            directory_generation: 0,
            tile_generation: 0,
            texture_serial: 0,
            last_viewport: Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)),
            window_title: String::new(),
        };

        if let Some(path) = initial_path {
            app.open_path(path);
        }
        Ok(app)
    }

    fn open_path(&mut self, path: PathBuf) {
        if !path.is_file() {
            self.session.begin(path.clone());
            self.loading_current = false;
            self.error = Some(format!(
                "The selected image is missing, inaccessible, or not a file: {}",
                path.display()
            ));
            self.warning = None;
            return;
        }

        self.session.begin(path.clone());
        self.loader.cancel_all();
        self.tile_generation = self.tile_generation.wrapping_add(1);
        self.tile_service
            .cancel_path_and_older(&path, self.tile_generation);
        self.tile_cache.remove_path(&path);
        self.tile_disabled = false;
        self.view = ViewState::with_mode(
            self.settings.zoom_mode == ZoomMode::Fit,
            self.settings.manual_zoom,
        );
        self.error = None;
        self.warning = None;
        self.select_snapshot_path(&path);
        let directory = parent_directory(&path);
        if self
            .snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.directory() != directory)
        {
            self.request_directory_scan(path.clone());
        }

        if let Some(preview) = self.cache.get(&path) {
            self.warning = icc_warning(preview.image.icc_status, self.ui_language);
            self.loading_current = needs_quality_upgrade(preview);
            if self.loading_current {
                self.request_current();
            }
            self.cache.mark_used(&path);
        } else {
            self.request_current();
        }
        self.request_preloads();
    }

    fn reload_current(&mut self) {
        let Some(path) = self.session.current_path().map(Path::to_path_buf) else {
            return;
        };
        self.session.begin(path.clone());
        self.loader.cancel_all();
        self.tile_generation = self.tile_generation.wrapping_add(1);
        self.tile_service
            .cancel_path_and_older(&path, self.tile_generation);
        self.tile_cache.remove_path(&path);
        self.tile_disabled = false;
        self.error = None;
        self.warning = None;
        self.loading_current = true;
        self.request_directory_scan(path);
        self.request_current();
        self.request_preloads();
    }

    fn select_snapshot_path(&mut self, path: &Path) {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return;
        };
        let Some(index) = snapshot
            .files()
            .iter()
            .position(|candidate| candidate.file_name() == path.file_name())
        else {
            self.snapshot = None;
            return;
        };
        snapshot.set_selected_index(index);
    }

    fn request_directory_scan(&mut self, selected: PathBuf) {
        let directory = parent_directory(&selected).to_path_buf();
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.directory() != directory)
        {
            self.snapshot = None;
            self.thumbnail_cache.retain_directory(&directory);
        }
        self.directory_generation = self.directory_generation.wrapping_add(1);
        self.thumbnail_service
            .cancel_generation(self.directory_generation.wrapping_sub(1));
        self.thumbnail_failures.clear();
        self.directory_error = None;
        if !self
            .directory_service
            .request(selected, self.directory_generation)
        {
            self.directory_error =
                Some("The directory scanner is busy; retry in a moment.".to_owned());
        }
    }

    fn refresh_directory(&mut self) {
        let Some(path) = self.session.current_path().map(Path::to_path_buf) else {
            return;
        };
        self.request_directory_scan(path);
    }

    fn request_current(&mut self) {
        let Some(path) = self.session.current_path().map(Path::to_path_buf) else {
            return;
        };
        self.loading_current = true;
        if !self
            .loader
            .request(path, self.session.generation(), LoadOptions::default())
        {
            self.loading_current = false;
            self.error = Some("The background loader is unavailable.".to_owned());
        }
    }

    fn request_preloads(&mut self) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let paths = [
            snapshot.previous().map(Path::to_path_buf),
            snapshot.next().map(Path::to_path_buf),
        ];
        let options = LoadOptions {
            max_preview_side: PRELOAD_SIDE,
            ..LoadOptions::default()
        };

        for path in paths.into_iter().flatten() {
            if !self.cache.contains(&path) && !self.loader.is_pending(&path) {
                self.loader
                    .request(path, self.session.generation(), options.clone());
            }
        }
    }

    fn navigate(&mut self, direction: i8) {
        let target = self.snapshot.as_ref().and_then(|snapshot| {
            if direction < 0 {
                snapshot.previous()
            } else {
                snapshot.next()
            }
        });
        if let Some(path) = target.map(Path::to_path_buf) {
            self.open_path(path);
        }
    }

    fn open_dialog(&mut self) {
        let tr = texts(self.ui_language);
        let mut dialog =
            rfd::FileDialog::new().add_filter(tr.filter_images, &["jpg", "jpeg", "png", "webp"]);
        if let Some(parent) = self
            .session
            .current_path()
            .and_then(Path::parent)
            .filter(|parent| parent.is_dir())
        {
            dialog = dialog.set_directory(parent);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(path);
        }
    }

    fn process_loader_results(&mut self, ctx: &egui::Context) {
        while let Some(response) = self.loader.poll() {
            if !self.session.is_current_generation(response.generation) {
                continue;
            }

            let is_current = self.session.current_path() == Some(response.path.as_path());
            match response.result {
                Ok(image) => {
                    let icc_status = image.icc_status;
                    let path = image.path.clone();
                    if let Err(message) = self.store_image(ctx, image) {
                        if is_current {
                            self.loading_current = false;
                            self.error = Some(message);
                        }
                        continue;
                    }

                    if is_current {
                        self.loading_current = false;
                        self.error = None;
                        self.warning = icc_warning(icc_status, self.ui_language);
                        self.cache.mark_used(&path);
                        self.request_preloads();
                    }
                }
                Err(message) => {
                    if is_current {
                        self.loading_current = false;
                        if self.cache.contains(&response.path) {
                            self.warning =
                                Some(format!("Full-size preview unavailable: {message}"));
                        } else {
                            self.error = Some(message);
                        }
                    }
                }
            }
        }
    }

    fn process_directory_results(&mut self) {
        while let Some(response) = self.directory_service.poll() {
            if response.generation != self.directory_generation {
                continue;
            }
            let directory = parent_directory(&response.selected).to_path_buf();
            if self
                .session
                .current_path()
                .map(parent_directory)
                .is_none_or(|current| current != directory)
            {
                continue;
            }
            match response.result {
                Ok(mut snapshot) => {
                    if snapshot.directory() != directory {
                        self.directory_error =
                            Some("The directory scan returned an unexpected path.".to_owned());
                        continue;
                    }
                    if let Some(path) = self.session.current_path().map(Path::to_path_buf)
                        && let Some(index) = snapshot
                            .files()
                            .iter()
                            .position(|candidate| candidate.file_name() == path.file_name())
                    {
                        snapshot.set_selected_index(index);
                    }
                    self.thumbnail_cache.retain_directory(snapshot.directory());
                    self.snapshot = Some(snapshot);
                    self.directory_error = None;
                    self.request_preloads();
                }
                Err(error) => {
                    self.directory_error = Some(error.to_string());
                }
            }
        }
    }

    fn process_thumbnail_results(&mut self, ctx: &egui::Context) {
        let mut uploaded = 0;
        while uploaded < MAX_THUMBNAIL_UPLOAD_PER_FRAME {
            let Some(response) = self.thumbnail_service.poll() else {
                break;
            };
            if response.generation != self.directory_generation {
                continue;
            }
            match response.result {
                Ok(mut image) => {
                    let [width, height] = image.preview_dimensions;
                    let Some(expected) = (width as usize)
                        .checked_mul(height as usize)
                        .and_then(|pixels| pixels.checked_mul(4))
                    else {
                        self.thumbnail_failures.insert(response.path);
                        continue;
                    };
                    if width == 0 || height == 0 || image.rgba.len() != expected {
                        self.thumbnail_failures.insert(response.path);
                        continue;
                    }
                    uploaded = uploaded.saturating_add(expected);
                    self.texture_serial = self.texture_serial.wrapping_add(1);
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &image.rgba,
                    );
                    let texture = ctx.load_texture(
                        format!("liteview-thumbnail-{}", self.texture_serial),
                        color_image,
                        TextureOptions::LINEAR,
                    );
                    image.rgba.clear();
                    self.thumbnail_failures.remove(&response.path);
                    self.thumbnail_cache.insert(
                        response.path,
                        CachedThumbnail {
                            texture,
                            bytes: expected,
                        },
                    );
                }
                Err(_) => {
                    self.thumbnail_failures.insert(response.path);
                }
            }
        }
    }

    fn process_tile_results(&mut self, ctx: &egui::Context) {
        let mut uploaded = 0;
        while uploaded < MAX_TILE_UPLOAD_PER_FRAME {
            let Some(response) = self.tile_service.poll() else {
                break;
            };
            let Some(path) = self.session.current_path() else {
                continue;
            };
            if response.key.generation != self.tile_generation || response.key.path != path {
                continue;
            }
            let Some(image) = self.current_image() else {
                continue;
            };
            let grid = TileGrid::new(image.image.display_dimensions, response.key.coord.level);
            let output = grid.output_size(response.key.coord);
            let Some(source_rect) = grid.source_rect(response.key.coord) else {
                continue;
            };
            let expected = match (output[0] as usize)
                .checked_mul(output[1] as usize)
                .and_then(|pixels| pixels.checked_mul(4))
            {
                Some(expected) => expected,
                None => continue,
            };
            match response.result {
                Ok(rgba) if output[0] > 0 && output[1] > 0 && rgba.len() == expected => {
                    uploaded = uploaded.saturating_add(expected);
                    self.texture_serial = self.texture_serial.wrapping_add(1);
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [output[0] as usize, output[1] as usize],
                        &rgba,
                    );
                    let texture = ctx.load_texture(
                        format!("liteview-tile-{}", self.texture_serial),
                        color_image,
                        TextureOptions::LINEAR,
                    );
                    self.tile_cache.insert(
                        TileCacheKey {
                            path: path.to_path_buf(),
                            coord: response.key.coord,
                        },
                        CachedTile {
                            texture,
                            source_rect: [
                                source_rect.x,
                                source_rect.y,
                                source_rect.width,
                                source_rect.height,
                            ],
                            bytes: expected,
                        },
                    );
                }
                Ok(_) => {
                    self.tile_disabled = true;
                    self.warning =
                        Some("A detail tile returned an invalid pixel buffer.".to_owned());
                }
                Err(message) => {
                    self.tile_disabled = true;
                    self.warning = Some(format!("Detail tiles unavailable: {message}"));
                }
            }
        }
    }

    fn store_image(&mut self, ctx: &egui::Context, mut image: LoadedImage) -> Result<(), String> {
        let [width, height] = image.preview_dimensions;
        if width == 0 || height == 0 {
            return Err("The decoded image has invalid dimensions.".to_owned());
        }
        let expected_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "The decoded image is too large to upload.".to_owned())?;
        if image.rgba.len() != expected_len {
            return Err("The decoder returned an invalid pixel buffer.".to_owned());
        }

        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            &image.rgba,
        );
        self.texture_serial = self.texture_serial.wrapping_add(1);
        let texture = ctx.load_texture(
            format!("liteview-preview-{}", self.texture_serial),
            color_image,
            TextureOptions::LINEAR,
        );
        let tile_icc_profile = image.icc_profile.take().map(std::sync::Arc::<[u8]>::from);
        image.rgba = Vec::new();
        if self.session.current_path() == Some(image.path.as_path()) {
            self.tile_generation = self.tile_generation.wrapping_add(1);
            self.tile_service
                .cancel_path_and_older(&image.path, self.tile_generation);
            self.tile_cache.remove_path(&image.path);
            self.tile_disabled = false;
        }
        self.cache.insert(
            image.path.clone(),
            CachedPreview {
                image,
                tile_icc_profile,
                texture,
                bytes: expected_len,
            },
            self.session.current_path(),
        );
        Ok(())
    }

    fn handle_global_input(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .first()
                .map(|file| file.path().to_path_buf())
        });
        if let Some(path) = dropped {
            self.open_path(path);
            return;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if self.settings_draft.is_some() {
                self.settings_draft = None;
            } else if self.metadata_visible {
                self.metadata_visible = false;
                self.settings.show_metadata = false;
                self.save_preferences();
            } else if self.fullscreen {
                self.toggle_fullscreen(ctx);
            }
            return;
        }

        if self.settings_draft.is_some() {
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (
            open,
            previous,
            next,
            zoom_in,
            zoom_out,
            fit,
            actual,
            rotate,
            fullscreen,
            info,
            directory,
            settings,
            open_selected,
        ) = ctx.input(|input| {
            (
                input.modifiers.command && input.key_pressed(egui::Key::O),
                input.key_pressed(egui::Key::ArrowLeft),
                input.key_pressed(egui::Key::ArrowRight),
                input.key_pressed(egui::Key::Plus),
                input.key_pressed(egui::Key::Minus),
                input.key_pressed(egui::Key::Num0),
                input.key_pressed(egui::Key::Num1),
                input.key_pressed(egui::Key::R),
                input.key_pressed(egui::Key::F),
                input.key_pressed(egui::Key::I),
                input.key_pressed(egui::Key::D),
                input.modifiers.command
                    && input.modifiers.shift
                    && input.key_pressed(egui::Key::Comma),
                input.key_pressed(egui::Key::Enter),
            )
        });

        if open {
            self.open_dialog();
        } else if self.settings.directory_open && (previous || next) {
            self.move_directory_selection(if previous { -1 } else { 1 });
        } else if previous {
            self.navigate(-1);
        } else if next {
            self.navigate(1);
        }
        if open_selected
            && self.settings.directory_open
            && let Some(path) = self
                .snapshot
                .as_ref()
                .and_then(DirectorySnapshot::current)
                .map(Path::to_path_buf)
        {
            self.settings.directory_open = false;
            self.save_preferences();
            self.open_path(path);
        }

        if zoom_in {
            self.zoom_by(1.25, ctx);
        }
        if zoom_out {
            self.zoom_by(1.0 / 1.25, ctx);
        }
        if fit {
            self.fit_to_window();
        }
        if actual {
            self.actual_size(ctx);
        }
        if rotate {
            self.rotate_clockwise(ctx);
        }
        if info {
            self.toggle_metadata();
        }
        if directory {
            self.set_directory_open(!self.settings.directory_open);
        }
        if settings {
            self.settings_draft = Some(self.settings.clone());
        }
        if fullscreen {
            self.toggle_fullscreen(ctx);
        }
    }

    fn move_directory_selection(&mut self, amount: isize) {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return;
        };
        let index = snapshot.selected_index().saturating_add_signed(amount);
        if index < snapshot.files().len() {
            snapshot.set_selected_index(index);
        }
    }

    fn toggle_metadata(&mut self) {
        self.metadata_visible = !self.metadata_visible;
        self.settings.show_metadata = self.metadata_visible;
        self.save_preferences();
    }

    fn set_directory_open(&mut self, open: bool) {
        self.settings.directory_open = open;
        if open {
            if self.snapshot.is_none()
                && let Some(path) = self.session.current_path().map(Path::to_path_buf)
            {
                self.request_directory_scan(path);
            }
        } else {
            self.thumbnail_service
                .cancel_generation(self.directory_generation);
            if let Some(path) = self.session.current_path().map(Path::to_path_buf) {
                let visible = HashSet::new();
                self.tile_service
                    .retain_path_coords(&path, self.tile_generation, &visible);
                self.tile_cache.set_protected(&path, &visible);
            }
        }
        self.save_preferences();
    }

    fn remember_view(&mut self) {
        self.settings.zoom_mode = if self.view.is_fit() {
            ZoomMode::Fit
        } else {
            ZoomMode::Manual
        };
        self.settings.manual_zoom = self.view.manual_zoom();
    }

    fn save_preferences(&mut self) {
        if let Err(error) = self.settings_store.save(&self.settings) {
            self.warning = Some(format!(
                "Could not save settings to {}: {error}",
                self.settings_store.path().display()
            ));
        }
    }

    fn zoom_by(&mut self, factor: f32, ctx: &egui::Context) {
        let Some(image) = self.current_image() else {
            return;
        };
        let viewport = self.last_viewport;
        let current = self.view.effective_zoom(
            image.image.display_dimensions,
            viewport.size(),
            ctx.pixels_per_point(),
        );
        self.view.zoom_at(
            current * factor,
            viewport.center(),
            viewport,
            image.image.display_dimensions,
            ctx.pixels_per_point(),
        );
        self.remember_view();
        self.save_preferences();
    }

    fn fit_to_window(&mut self) {
        self.view.fit();
        self.remember_view();
        self.save_preferences();
    }

    fn actual_size(&mut self, ctx: &egui::Context) {
        self.view.actual_size();
        if let Some(image) = self.current_image() {
            self.view.clamp_pan(
                image.image.display_dimensions,
                self.last_viewport.size(),
                ctx.pixels_per_point(),
            );
        }
        self.remember_view();
        self.save_preferences();
    }

    fn rotate_clockwise(&mut self, ctx: &egui::Context) {
        self.view.rotate_clockwise();
        if let Some(image) = self.current_image() {
            self.view.clamp_pan(
                image.image.display_dimensions,
                self.last_viewport.size(),
                ctx.pixels_per_point(),
            );
        }
        self.remember_view();
        self.save_preferences();
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
    }

    fn apply_settings(&mut self, mut settings: AppSettings) {
        settings.normalize();
        let previous_zoom = self.settings.zoom_mode;
        let previous_manual_zoom = self.settings.manual_zoom;
        self.settings = settings.clone();
        self.metadata_visible = settings.show_metadata;
        let locale = sys_locale::get_locale();
        self.ui_language = settings.language.resolve(locale.as_deref());
        if settings.zoom_mode != previous_zoom
            || (settings.zoom_mode == ZoomMode::Manual
                && (settings.manual_zoom - previous_manual_zoom).abs() > f32::EPSILON)
        {
            match settings.zoom_mode {
                ZoomMode::Fit => self.view.fit(),
                ZoomMode::Manual => self.view.set_manual_zoom(settings.manual_zoom),
            }
        }
        self.apply_cache_budget(settings.cache_limit_mib);
        if settings.directory_open
            && self.snapshot.is_none()
            && let Some(path) = self.session.current_path().map(Path::to_path_buf)
        {
            self.request_directory_scan(path);
        }
        self.settings_draft = None;
        self.save_preferences();
    }

    fn apply_cache_budget(&mut self, limit_mib: u32) {
        let budget = CacheBudget::for_limit(limit_mib);
        self.cache.set_limits(
            budget.preview,
            CACHE_MAX_ENTRIES,
            self.session.current_path(),
        );
        self.thumbnail_cache.set_max_bytes(budget.thumbnails);
        self.tile_cache.set_max_bytes(budget.tiles);
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.settings_draft.take() else {
            return;
        };
        let tr = texts(self.ui_language);
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;

        egui::Window::new(tr.settings_window)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                egui::ComboBox::from_label(tr.language)
                    .selected_text(match draft.language {
                        Language::System => tr.language_system,
                        Language::English => tr.language_english,
                        Language::Chinese => tr.language_chinese,
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.language,
                            Language::System,
                            tr.language_system,
                        );
                        ui.selectable_value(
                            &mut draft.language,
                            Language::English,
                            tr.language_english,
                        );
                        ui.selectable_value(
                            &mut draft.language,
                            Language::Chinese,
                            tr.language_chinese,
                        );
                    });

                ui.heading(tr.viewing);
                egui::ComboBox::from_label(tr.zoom_mode)
                    .selected_text(match draft.zoom_mode {
                        ZoomMode::Fit => tr.fit_to_window,
                        ZoomMode::Manual => tr.manual_zoom,
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.zoom_mode, ZoomMode::Fit, tr.fit_to_window);
                        ui.selectable_value(&mut draft.zoom_mode, ZoomMode::Manual, tr.manual_zoom);
                    });
                ui.add_enabled(
                    draft.zoom_mode == ZoomMode::Manual,
                    egui::Slider::new(&mut draft.manual_zoom, 0.01..=32.0)
                        .logarithmic(true)
                        .text(tr.manual_zoom),
                );
                ui.checkbox(&mut draft.show_metadata, tr.show_image_info);
                ui.checkbox(&mut draft.directory_open, tr.open_directory_browser);
                egui::ComboBox::from_label(tr.directory_layout)
                    .selected_text(match draft.directory_view {
                        DirectoryView::Grid => tr.thumbnail_grid,
                        DirectoryView::List => tr.compact_list,
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.directory_view,
                            DirectoryView::Grid,
                            tr.thumbnail_grid,
                        );
                        ui.selectable_value(
                            &mut draft.directory_view,
                            DirectoryView::List,
                            tr.compact_list,
                        );
                    });

                ui.add_space(10.0);
                ui.heading(tr.resource_limits);
                ui.add(
                    egui::Slider::new(
                        &mut draft.cache_limit_mib,
                        MIN_CACHE_LIMIT_MIB..=MAX_CACHE_LIMIT_MIB,
                    )
                    .logarithmic(true)
                    .suffix(" MiB")
                    .text(tr.gpu_cache_budget),
                );
                ui.colored_label(
                    MUTED,
                    format!(
                        "{} {}",
                        tr.settings_file,
                        self.settings_store.path().display()
                    ),
                );

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(tr.apply)
                        .on_hover_text(tr.apply_tooltip)
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui
                        .button(tr.cancel)
                        .on_hover_text(tr.cancel_tooltip)
                        .clicked()
                    {
                        cancel = true;
                    }
                });
            });

        if apply {
            self.apply_settings(draft);
        } else if cancel || !open {
            self.settings_draft = None;
        } else {
            self.settings_draft = Some(draft);
        }
    }

    fn update_window_title(&mut self, ctx: &egui::Context) {
        let title = self
            .session
            .current_path()
            .and_then(Path::file_name)
            .map(|name| format!("{} - LiteView", name.to_string_lossy()))
            .unwrap_or_else(|| "LiteView".to_owned());
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn toolbar_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has_previous = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.previous().is_some());
        let has_next = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.next().is_some());
        let has_image = self.current_image().is_some();
        let has_path = self.session.current_path().is_some();

        let tr = texts(self.ui_language);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if ui.button(tr.open).on_hover_text(tr.open_tooltip).clicked() {
                self.open_dialog();
            }
            if ui
                .add_enabled(
                    has_path,
                    egui::Button::new(if self.error.is_some() {
                        tr.retry
                    } else {
                        tr.reload
                    }),
                )
                .on_hover_text(if self.error.is_some() {
                    tr.retry
                } else {
                    tr.reload_tooltip
                })
                .clicked()
            {
                self.reload_current();
            }
            if ui
                .selectable_label(self.settings.directory_open, tr.directory)
                .on_hover_text(tr.directory_tooltip)
                .clicked()
            {
                self.set_directory_open(!self.settings.directory_open);
            }
            if ui
                .button(tr.settings)
                .on_hover_text(tr.settings_tooltip)
                .clicked()
            {
                self.settings_draft = Some(self.settings.clone());
            }
            ui.separator();
            if ui
                .add_enabled(has_previous, egui::Button::new("<"))
                .on_hover_text(tr.previous_tooltip)
                .clicked()
            {
                self.navigate(-1);
            }
            if ui
                .add_enabled(has_next, egui::Button::new(">"))
                .on_hover_text(tr.next_tooltip)
                .clicked()
            {
                self.navigate(1);
            }
            ui.separator();
            if ui
                .add_enabled(has_image, egui::Button::new(tr.fit))
                .on_hover_text(tr.fit_tooltip)
                .clicked()
            {
                self.fit_to_window();
            }
            if ui
                .add_enabled(has_image, egui::Button::new(tr.actual_size))
                .on_hover_text(tr.actual_size_tooltip)
                .clicked()
            {
                self.actual_size(ctx);
            }
            if ui
                .add_enabled(has_image, egui::Button::new(tr.rotate))
                .on_hover_text(tr.rotate_tooltip)
                .clicked()
            {
                self.rotate_clockwise(ctx);
            }
            ui.separator();
            if ui
                .selectable_label(self.metadata_visible, tr.info)
                .on_hover_text(tr.info_tooltip)
                .clicked()
            {
                self.toggle_metadata();
            }

            if let Some(path) = self.session.current_path() {
                ui.separator();
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                ui.add(Label::new(RichText::new(name).strong()).truncate())
                    .on_hover_text(path.display().to_string());
            }
        });
    }

    fn status_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tr = texts(self.ui_language);
        ui.horizontal(|ui| {
            if let Some(message) = self.warning.as_deref() {
                ui.colored_label(WARNING, message);
            } else if let Some(image) = self.current_image() {
                let [width, height] = image.image.display_dimensions;
                ui.label(format!(
                    "{}  {}x{}  {}",
                    image.image.format.label(),
                    width,
                    height,
                    format_size(image.image.file_size)
                ));
            } else if self.loading_current {
                ui.label(tr.loading_image);
            } else {
                ui.colored_label(MUTED, tr.no_image_open);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(image) = self.current_image() {
                    let zoom = self.view.effective_zoom(
                        image.image.display_dimensions,
                        self.last_viewport.size(),
                        ctx.pixels_per_point(),
                    );
                    ui.label(format!("{:.0}%", zoom * 100.0));
                    if image.image.downsampled {
                        ui.colored_label(MUTED, tr.preview);
                    }
                    if let Some(snapshot) = self.snapshot.as_ref() {
                        ui.label(format!(
                            "{} / {}",
                            snapshot.selected_index() + 1,
                            snapshot.files().len()
                        ));
                    }
                }
            });
        });
    }

    fn metadata_ui(&mut self, ui: &mut egui::Ui) {
        let tr = texts(self.ui_language);
        egui::Panel::right("image-info")
            .resizable(false)
            .default_size(270.0)
            .size_range(220.0..=360.0)
            .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(tr.image_info);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button(tr.close)
                            .on_hover_text(tr.close_tooltip)
                            .clicked()
                        {
                            self.metadata_visible = false;
                            self.settings.show_metadata = false;
                            self.save_preferences();
                        }
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let Some(image) = self.current_image() else {
                        ui.colored_label(MUTED, tr.no_image_selected);
                        return;
                    };
                    let info = &image.image;
                    detail(ui, tr.file, &info.path.display().to_string());
                    detail(ui, tr.format, info.format.label());
                    detail(
                        ui,
                        tr.source,
                        &format!(
                            "{} x {}",
                            info.source_dimensions[0], info.source_dimensions[1]
                        ),
                    );
                    detail(
                        ui,
                        tr.display,
                        &format!(
                            "{} x {}",
                            info.display_dimensions[0], info.display_dimensions[1]
                        ),
                    );
                    detail(
                        ui,
                        tr.preview_size,
                        &format!(
                            "{} x {}",
                            info.preview_dimensions[0], info.preview_dimensions[1]
                        ),
                    );
                    detail(ui, tr.file_size, &format_size(info.file_size));
                    detail(ui, tr.decode, &format_duration(info.elapsed));
                    detail(ui, tr.color, icc_label(info.icc_status, self.ui_language));
                    ui.add_space(12.0);
                    ui.heading(tr.metadata);
                    ui.add_space(6.0);
                    metadata_detail(ui, tr.camera, info.metadata.camera.as_deref());
                    metadata_detail(ui, tr.captured, info.metadata.captured_at.as_deref());
                    metadata_detail(ui, tr.exposure, info.metadata.exposure.as_deref());
                    metadata_detail(ui, tr.aperture, info.metadata.aperture.as_deref());
                    metadata_detail(ui, tr.iso, info.metadata.iso.as_deref());
                    metadata_detail(ui, tr.lens, info.metadata.lens.as_deref());
                });
            });
    }

    fn directory_ui(&mut self, ui: &mut egui::Ui) {
        let tr = texts(self.ui_language);
        ui.painter()
            .rect_filled(ui.available_rect_before_wrap(), 0.0, VIEWER_BG);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let count = self
                .snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.files().len());
            ui.heading(tr.current_directory);
            ui.colored_label(MUTED, tr.images_count(count));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .button(tr.refresh)
                    .on_hover_text(tr.refresh_tooltip)
                    .clicked()
                {
                    self.refresh_directory();
                }
                let list_selected = self.settings.directory_view == DirectoryView::List;
                if ui
                    .selectable_label(list_selected, tr.list)
                    .on_hover_text(tr.list_tooltip)
                    .clicked()
                {
                    self.settings.directory_view = DirectoryView::List;
                    self.save_preferences();
                }
                let grid_selected = self.settings.directory_view == DirectoryView::Grid;
                if ui
                    .selectable_label(grid_selected, tr.grid)
                    .on_hover_text(tr.grid_tooltip)
                    .clicked()
                {
                    self.settings.directory_view = DirectoryView::Grid;
                    self.save_preferences();
                }
                if ui
                    .button(tr.view_selected)
                    .on_hover_text(tr.view_selected_tooltip)
                    .clicked()
                    && let Some(path) = self
                        .snapshot
                        .as_ref()
                        .and_then(DirectorySnapshot::current)
                        .map(Path::to_path_buf)
                {
                    self.settings.directory_open = false;
                    self.save_preferences();
                    self.open_path(path);
                }
            });
        });
        if let Some(snapshot) = self.snapshot.as_ref() {
            ui.add(
                Label::new(RichText::new(snapshot.directory().display().to_string()).color(MUTED))
                    .truncate(),
            );
        }
        ui.separator();

        if self.session.current_path().is_none() {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                ui.heading(tr.no_directory_selected);
                ui.colored_label(MUTED, tr.open_to_browse);
                if ui
                    .button(tr.open_image)
                    .on_hover_text(tr.open_tooltip)
                    .clicked()
                {
                    self.open_dialog();
                }
            });
            return;
        }

        let Some(snapshot) = self.snapshot.as_ref() else {
            if let Some(error) = self.directory_error.as_deref() {
                let error = error.to_owned();
                ui.add_space(28.0);
                ui.vertical_centered(|ui| {
                    ui.heading(tr.directory_unavailable);
                    ui.colored_label(MUTED, error);
                    if ui
                        .button(tr.retry)
                        .on_hover_text(tr.refresh_tooltip)
                        .clicked()
                    {
                        self.refresh_directory();
                    }
                });
            } else {
                ui.add_space(28.0);
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.colored_label(MUTED, tr.scanning);
                });
            }
            return;
        };

        let files = snapshot.files().to_vec();
        if files.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(28.0);
                ui.colored_label(MUTED, tr.no_supported_images);
            });
            return;
        }
        let selected_index = snapshot.selected_index();
        let mut next_selected = selected_index;
        let mut activate = None;
        let mut visible_paths = HashSet::new();
        let generation = self.directory_generation;
        let mut textures = Vec::with_capacity(files.len());
        for path in &files {
            textures.push(
                self.thumbnail_cache
                    .get(path)
                    .map(|thumbnail| thumbnail.texture.clone()),
            );
        }

        match self.settings.directory_view {
            DirectoryView::Grid => {
                let columns = ((ui.available_width() + 8.0) / (THUMBNAIL_CARD_WIDTH + 8.0))
                    .floor()
                    .max(1.0) as usize;
                let rows = files.len().div_ceil(columns);
                egui::ScrollArea::vertical()
                    .id_salt("directory-grid")
                    .auto_shrink([false, false])
                    .show_rows(ui, THUMBNAIL_ROW_HEIGHT, rows, |ui, row_range| {
                        for row in row_range {
                            ui.horizontal(|ui| {
                                for column in 0..columns {
                                    let index = row * columns + column;
                                    let Some(path) = files.get(index) else {
                                        continue;
                                    };
                                    visible_paths.insert(path.clone());
                                    let response = draw_thumbnail_card(
                                        ui,
                                        path,
                                        index == selected_index,
                                        textures[index].clone(),
                                        self.thumbnail_failures.contains(path),
                                        tr.thumbnail_unavailable,
                                        tr.loading,
                                    );
                                    if response.clicked() {
                                        next_selected = index;
                                    }
                                    if response.double_clicked() {
                                        activate = Some(index);
                                    }
                                }
                            });
                        }
                    });
            }
            DirectoryView::List => {
                egui::ScrollArea::vertical()
                    .id_salt("directory-list")
                    .auto_shrink([false, false])
                    .show_rows(ui, 54.0, files.len(), |ui, rows| {
                        for index in rows {
                            let path = &files[index];
                            visible_paths.insert(path.clone());
                            let response = draw_thumbnail_row(
                                ui,
                                path,
                                index == selected_index,
                                textures[index].clone(),
                                self.thumbnail_failures.contains(path),
                                tr.thumbnail_unavailable,
                                tr.loading,
                            );
                            if response.clicked() {
                                next_selected = index;
                            }
                            if response.double_clicked() {
                                activate = Some(index);
                            }
                        }
                    });
            }
        }

        if next_selected != selected_index
            && let Some(snapshot) = self.snapshot.as_mut()
        {
            snapshot.set_selected_index(next_selected);
        }
        self.thumbnail_service.retain(generation, &visible_paths);
        for path in &visible_paths {
            if self.thumbnail_cache.contains(path) {
                self.thumbnail_cache.mark_used(path);
            } else if !self.thumbnail_failures.contains(path)
                && !self.thumbnail_service.is_pending(path)
            {
                self.thumbnail_service.request(
                    path.clone(),
                    generation,
                    ThumbnailPriority::Visible,
                );
            }
        }

        if let Some(index) = activate.filter(|index| *index < files.len()) {
            self.settings.directory_open = false;
            self.save_preferences();
            self.open_path(files[index].clone());
        }
    }

    fn draw_detail_tiles(
        &mut self,
        painter: &egui::Painter,
        viewport: Rect,
        image_center: Pos2,
        display_size: Vec2,
    ) {
        if self.tile_disabled {
            return;
        }
        let Some((
            path,
            format,
            image_pixels,
            preview_dimensions,
            source_dimensions,
            exif_orientation,
            icc_profile,
        )) = self.current_image().map(|image| {
            (
                image.image.path.clone(),
                image.image.format,
                image.image.display_dimensions,
                image.image.preview_dimensions,
                image.image.source_dimensions,
                image.image.exif_orientation,
                image.tile_icc_profile.clone(),
            )
        })
        else {
            return;
        };
        if format != SourceFormat::Jpeg || preview_dimensions == image_pixels {
            return;
        }

        let zoom = self.view.effective_zoom(
            image_pixels,
            viewport.size(),
            painter.ctx().pixels_per_point(),
        );
        let preview_scale = [
            preview_dimensions[0] as f32 / image_pixels[0].max(1) as f32,
            preview_dimensions[1] as f32 / image_pixels[1].max(1) as f32,
        ]
        .into_iter()
        .fold(0.0_f32, f32::max);
        if zoom + 0.001 < preview_scale {
            let visible = HashSet::new();
            self.tile_service
                .retain_path_coords(&path, self.tile_generation, &visible);
            self.tile_cache.set_protected(&path, &visible);
            return;
        }

        let level = level_for_zoom(zoom);
        let grid = TileGrid::new(image_pixels, level);
        let visible_rect = visible_source_rect(
            viewport,
            image_center,
            display_size,
            self.view.rotation_radians(),
            image_pixels,
            128,
        );
        let visible_tiles: HashSet<_> = grid.visible_tiles(visible_rect).into_iter().collect();
        self.tile_service
            .retain_path_coords(&path, self.tile_generation, &visible_tiles);
        self.tile_cache.set_protected(&path, &visible_tiles);
        if visible_tiles.is_empty() {
            return;
        }

        let mut used = Vec::new();
        for coord in &visible_tiles {
            let key = TileCacheKey {
                path: path.clone(),
                coord: *coord,
            };
            if let Some(tile) = self.tile_cache.get(&key) {
                draw_textured_tile(
                    painter,
                    viewport,
                    image_center,
                    display_size,
                    self.view.rotation_radians(),
                    TileGeometry {
                        source_rect: tile.source_rect,
                        image_pixels,
                    },
                    tile.texture.id(),
                );
                used.push(key);
                continue;
            }

            let request_key = TileRequestKey {
                generation: self.tile_generation,
                path: path.clone(),
                coord: *coord,
            };
            if self.tile_service.is_pending(&request_key) {
                continue;
            }
            let Some(source_rect) = grid.source_rect(*coord) else {
                continue;
            };
            let request = TileDecodeRequest {
                path: path.clone(),
                source_dimensions,
                source_rect: [
                    source_rect.x,
                    source_rect.y,
                    source_rect.width,
                    source_rect.height,
                ],
                output_size: grid.output_size(*coord),
                exif_orientation,
                icc_profile: icc_profile.clone(),
            };
            self.tile_service.request(request, request_key);
        }
        for key in used {
            self.tile_cache.mark_used(&key);
        }
    }

    fn viewer_ui(&mut self, ui: &mut egui::Ui) {
        let tr = texts(self.ui_language);
        let viewport = ui.available_rect_before_wrap();
        let (rect, response) = ui.allocate_exact_size(viewport.size(), Sense::click_and_drag());
        self.last_viewport = rect;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, VIEWER_BG);

        let Some(image) = self.current_image() else {
            draw_empty_state(
                &painter,
                rect,
                if self.loading_current {
                    tr.loading_image
                } else if self.error.is_some() {
                    tr.unable_to_open
                } else {
                    tr.open_to_begin
                },
                self.error.as_deref().unwrap_or(tr.drop_hint),
            );
            return;
        };

        let texture_id = image.texture.id();
        let pixels = image.image.display_dimensions;
        self.handle_viewer_input(ui.ctx(), &response, rect, pixels);
        self.view
            .clamp_pan(pixels, rect.size(), ui.ctx().pixels_per_point());

        let size = self
            .view
            .displayed_size(pixels, rect.size(), ui.ctx().pixels_per_point());
        let image_rect = Rect::from_center_size(self.view.image_center(rect), size);
        draw_checkerboard(&painter, rect, image_rect);
        draw_textured_image(
            &painter,
            rect,
            self.view.image_center(rect),
            size,
            self.view.rotation_radians(),
            texture_id,
        );
        self.draw_detail_tiles(&painter, rect, self.view.image_center(rect), size);

        if size.x > 2.0 && size.y > 2.0 {
            painter.rect_stroke(
                image_rect,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(72, 76, 82)),
                StrokeKind::Inside,
            );
        }

        if self.loading_current {
            draw_loading_pill(&painter, rect, tr.loading);
        }
        if ui.ctx().input(|input| !input.raw.hovered_files.is_empty()) {
            draw_drop_overlay(&painter, rect, tr.drop_to_open);
        }
    }

    fn handle_viewer_input(
        &mut self,
        ctx: &egui::Context,
        response: &Response,
        viewport: Rect,
        pixels: [u32; 2],
    ) {
        if response.dragged_by(egui::PointerButton::Primary) {
            self.view.set_pan(self.view.pan() + response.drag_delta());
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }

        if response.double_clicked() {
            let zoom = self
                .view
                .effective_zoom(pixels, viewport.size(), ctx.pixels_per_point());
            if zoom > 0.999 {
                self.view.fit();
            } else {
                self.view.actual_size();
            }
            self.remember_view();
            self.save_preferences();
        }

        if !response.hovered() {
            return;
        }
        let (scroll, pinch_zoom) =
            ctx.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
        let factor = if (pinch_zoom - 1.0).abs() > 0.001 {
            pinch_zoom
        } else if scroll.abs() > 0.1 {
            (scroll * 0.0035).exp()
        } else {
            1.0
        };
        if (factor - 1.0).abs() < 0.001 {
            return;
        }

        let current = self
            .view
            .effective_zoom(pixels, viewport.size(), ctx.pixels_per_point());
        let cursor = ctx.pointer_hover_pos().unwrap_or(viewport.center());
        self.view.zoom_at(
            current * factor,
            cursor,
            viewport,
            pixels,
            ctx.pixels_per_point(),
        );
        self.remember_view();
    }

    fn current_image(&self) -> Option<&CachedPreview> {
        self.session
            .current_path()
            .and_then(|path| self.cache.get(path))
    }
}

impl eframe::App for LiteViewApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        VIEWER_BG.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.process_loader_results(&ctx);
        self.process_directory_results();
        self.process_thumbnail_results(&ctx);
        self.process_tile_results(&ctx);
        self.handle_global_input(&ctx);
        self.update_window_title(&ctx);

        egui::Panel::top("toolbar")
            .exact_size(TOOLBAR_HEIGHT)
            .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(8.0))
            .show(ui, |ui| self.toolbar_ui(ui, &ctx));

        egui::Panel::bottom("status")
            .exact_size(STATUS_HEIGHT)
            .frame(
                egui::Frame::NONE
                    .fill(STATUS_BG)
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show(ui, |ui| self.status_ui(ui, &ctx));

        if self.metadata_visible {
            self.metadata_ui(ui);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                if self.settings.directory_open {
                    self.directory_ui(ui);
                } else {
                    self.viewer_ui(ui);
                }
            });
        self.settings_window(&ctx);

        if self.loader.is_busy()
            || self.loading_current
            || self.thumbnail_service.active_len() > 0
            || self.thumbnail_service.queued_len() > 0
            || self.thumbnail_service.pending_len() > 0
            || self.tile_service.pending_count() > 0
            || (self.settings.directory_open
                && self.session.current_path().is_some()
                && self.snapshot.is_none()
                && self.directory_error.is_none())
        {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.remember_view();
        self.save_preferences();
    }

    fn on_exit(&mut self) {
        self.remember_view();
        self.save_preferences();
    }
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn needs_quality_upgrade(preview: &CachedPreview) -> bool {
    let [width, height] = preview.image.display_dimensions;
    capped_dimensions(width, height, LoadOptions::default().max_preview_side)
        != preview.image.preview_dimensions
}

fn icc_warning(status: IccStatus, language: UiLanguage) -> Option<String> {
    let tr = texts(language);
    (status == IccStatus::Invalid).then(|| tr.invalid_profile_warning.to_owned())
}

fn icc_label(status: IccStatus, language: UiLanguage) -> &'static str {
    let tr = texts(language);
    match status {
        IccStatus::Absent => tr.srgb_without_profile,
        IccStatus::Converted => tr.converted_to_srgb,
        IccStatus::Invalid => tr.invalid_profile,
    }
}

fn detail(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.add_space(4.0);
    ui.colored_label(MUTED, label);
    ui.add(Label::new(value).wrap());
}

fn metadata_detail(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    detail(ui, label, value.unwrap_or("Not available"));
}

fn draw_thumbnail_card(
    ui: &mut egui::Ui,
    path: &Path,
    selected: bool,
    texture: Option<egui::TextureHandle>,
    failed: bool,
    unavailable: &str,
    loading: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(THUMBNAIL_CARD_WIDTH, THUMBNAIL_CARD_HEIGHT),
        Sense::click(),
    );
    let painter = ui.painter_at(rect);
    let background = if selected {
        Color32::from_rgb(54, 67, 82)
    } else if response.hovered() {
        Color32::from_rgb(44, 48, 53)
    } else {
        Color32::from_rgb(37, 40, 44)
    };
    painter.rect_filled(rect, 5.0, background);
    if selected {
        painter.rect_stroke(
            rect,
            5.0,
            Stroke::new(1.5, Color32::from_rgb(116, 171, 235)),
            StrokeKind::Inside,
        );
    }

    let image_rect = Rect::from_min_size(
        rect.min + Vec2::new(9.0, 9.0),
        Vec2::new(THUMBNAIL_CARD_WIDTH - 18.0, 116.0),
    );
    paint_thumbnail_preview(&painter, image_rect, texture, failed, unavailable, loading);

    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    paint_clipped_label(
        &painter,
        Rect::from_min_size(
            Pos2::new(rect.min.x + 10.0, image_rect.max.y + 10.0),
            Vec2::new(rect.width() - 20.0, 20.0),
        ),
        &name,
        Color32::from_rgb(226, 229, 233),
    );
    paint_clipped_label(
        &painter,
        Rect::from_min_size(
            Pos2::new(rect.min.x + 10.0, image_rect.max.y + 34.0),
            Vec2::new(rect.width() - 20.0, 18.0),
        ),
        path.extension()
            .map(|value| value.to_string_lossy().to_ascii_uppercase())
            .as_deref()
            .unwrap_or("IMAGE"),
        MUTED,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(path.display().to_string())
}

fn draw_thumbnail_row(
    ui: &mut egui::Ui,
    path: &Path,
    selected: bool,
    texture: Option<egui::TextureHandle>,
    failed: bool,
    unavailable: &str,
    loading: &str,
) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 54.0), Sense::click());
    let painter = ui.painter_at(rect);
    let background = if selected {
        Color32::from_rgb(54, 67, 82)
    } else if response.hovered() {
        Color32::from_rgb(44, 48, 53)
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 4.0, background);
    if selected {
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Color32::from_rgb(116, 171, 235)),
            StrokeKind::Inside,
        );
    }

    let preview_rect = Rect::from_min_size(rect.min + Vec2::new(7.0, 6.0), Vec2::new(56.0, 42.0));
    paint_thumbnail_preview(
        &painter,
        preview_rect,
        texture,
        failed,
        unavailable,
        loading,
    );
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    paint_clipped_label(
        &painter,
        Rect::from_min_size(
            Pos2::new(preview_rect.max.x + 10.0, rect.min.y + 17.0),
            Vec2::new((rect.max.x - preview_rect.max.x - 20.0).max(0.0), 22.0),
        ),
        &name,
        Color32::from_rgb(226, 229, 233),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(path.display().to_string())
}

fn paint_thumbnail_preview(
    painter: &egui::Painter,
    rect: Rect,
    texture: Option<egui::TextureHandle>,
    failed: bool,
    unavailable: &str,
    loading: &str,
) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(28, 30, 33));
    if let Some(texture) = texture {
        let texture_size = texture.size_vec2();
        if texture_size.x > 0.0 && texture_size.y > 0.0 {
            let scale = (rect.width() / texture_size.x)
                .min(rect.height() / texture_size.y)
                .max(0.0);
            let image_rect = Rect::from_center_size(rect.center(), texture_size * scale);
            painter.image(
                texture.id(),
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if failed { unavailable } else { loading },
            egui::FontId::proportional(12.0),
            if failed { WARNING } else { MUTED },
        );
    }
}

fn paint_clipped_label(painter: &egui::Painter, rect: Rect, text: &str, color: Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(13.0), color);
    painter.with_clip_rect(rect).galley(
        Pos2::new(rect.min.x, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}

fn format_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2} s", duration.as_secs_f64())
    } else {
        format!("{} ms", duration.as_millis())
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL_BG;
    visuals.window_fill = PANEL_BG;
    visuals.faint_bg_color = Color32::from_rgb(42, 45, 50);
    visuals.extreme_bg_color = VIEWER_BG;
    visuals.selection.bg_fill = Color32::from_rgb(54, 103, 155);
    visuals.hyperlink_color = Color32::from_rgb(116, 171, 235);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(48, 52, 57);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(62, 68, 75);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(53, 96, 145);
    ctx.set_visuals(visuals);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(9.0, 5.0);
    });
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{CacheBudget, MIB, parent_directory};

    #[test]
    fn splits_cache_budget_across_all_texture_users() {
        let budget = CacheBudget::for_limit(256);
        assert_eq!(budget.preview, 64 * MIB);
        assert_eq!(budget.tiles, 160 * MIB);
        assert_eq!(budget.thumbnails, 32 * MIB);
        assert_eq!(budget.preview + budget.tiles + budget.thumbnails, 256 * MIB);
    }

    #[test]
    fn normalizes_empty_relative_parent_directories() {
        assert_eq!(parent_directory(Path::new("photo.jpg")), Path::new("."));
        assert_eq!(
            parent_directory(Path::new("album/photo.jpg")),
            Path::new("album")
        );
    }
}

use eframe::egui::{Pos2, Rect, Vec2};

const MIN_ZOOM: f32 = 0.01;
const MAX_ZOOM: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZoomMode {
    Fit,
    Manual,
}

#[derive(Debug, Clone)]
pub struct ViewState {
    mode: ZoomMode,
    manual_zoom: f32,
    pan: Vec2,
    quarter_turns: u8,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            mode: ZoomMode::Fit,
            manual_zoom: 1.0,
            pan: Vec2::ZERO,
            quarter_turns: 0,
        }
    }
}

impl ViewState {
    pub fn with_mode(fit: bool, manual_zoom: f32) -> Self {
        Self {
            mode: if fit { ZoomMode::Fit } else { ZoomMode::Manual },
            manual_zoom: clamp_zoom(manual_zoom),
            pan: Vec2::ZERO,
            quarter_turns: 0,
        }
    }

    pub fn fit(&mut self) {
        self.mode = ZoomMode::Fit;
        self.pan = Vec2::ZERO;
    }

    pub fn actual_size(&mut self) {
        self.mode = ZoomMode::Manual;
        self.manual_zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    pub fn set_manual_zoom(&mut self, zoom: f32) {
        self.mode = ZoomMode::Manual;
        self.manual_zoom = clamp_zoom(zoom);
    }

    pub fn is_fit(&self) -> bool {
        self.mode == ZoomMode::Fit
    }

    pub fn manual_zoom(&self) -> f32 {
        self.manual_zoom
    }

    pub fn pan(&self) -> Vec2 {
        self.pan
    }

    pub fn set_pan(&mut self, pan: Vec2) {
        self.pan = pan;
    }

    pub fn rotate_clockwise(&mut self) {
        self.quarter_turns = (self.quarter_turns + 1) % 4;
    }

    pub fn rotation_radians(&self) -> f32 {
        f32::from(self.quarter_turns) * std::f32::consts::FRAC_PI_2
    }

    pub fn effective_zoom(
        &self,
        image_pixels: [u32; 2],
        viewport: Vec2,
        pixels_per_point: f32,
    ) -> f32 {
        let [width, height] = self.rotated_pixels(image_pixels);
        if width == 0
            || height == 0
            || viewport.x <= 0.0
            || viewport.y <= 0.0
            || pixels_per_point <= 0.0
        {
            return 1.0;
        }

        match self.mode {
            ZoomMode::Fit => {
                let horizontal = viewport.x * pixels_per_point / width as f32;
                let vertical = viewport.y * pixels_per_point / height as f32;
                clamp_zoom(horizontal.min(vertical).min(1.0))
            }
            ZoomMode::Manual => self.manual_zoom,
        }
    }

    pub fn displayed_size(
        &self,
        image_pixels: [u32; 2],
        viewport: Vec2,
        pixels_per_point: f32,
    ) -> Vec2 {
        let [width, height] = self.rotated_pixels(image_pixels);
        let zoom = self.effective_zoom(image_pixels, viewport, pixels_per_point);
        Vec2::new(width as f32, height as f32) * zoom / pixels_per_point.max(f32::EPSILON)
    }

    pub fn image_center(&self, viewport: Rect) -> Pos2 {
        viewport.center() + self.pan
    }

    pub fn zoom_at(
        &mut self,
        target_zoom: f32,
        cursor: Pos2,
        viewport: Rect,
        image_pixels: [u32; 2],
        pixels_per_point: f32,
    ) {
        let old_size = self.displayed_size(image_pixels, viewport.size(), pixels_per_point);
        let old_center = self.image_center(viewport);
        let local = if old_size.x > 0.0 && old_size.y > 0.0 {
            Vec2::new(
                (cursor.x - old_center.x) / old_size.x,
                (cursor.y - old_center.y) / old_size.y,
            )
        } else {
            Vec2::ZERO
        };

        self.set_manual_zoom(target_zoom);
        let new_size = self.displayed_size(image_pixels, viewport.size(), pixels_per_point);
        let new_center = cursor - Vec2::new(local.x * new_size.x, local.y * new_size.y);
        self.pan = new_center - viewport.center();
        self.clamp_pan(image_pixels, viewport.size(), pixels_per_point);
    }

    pub fn clamp_pan(&mut self, image_pixels: [u32; 2], viewport: Vec2, pixels_per_point: f32) {
        let size = self.displayed_size(image_pixels, viewport, pixels_per_point);
        let max_x = ((size.x - viewport.x) * 0.5).max(0.0);
        let max_y = ((size.y - viewport.y) * 0.5).max(0.0);
        self.pan.x = self.pan.x.clamp(-max_x, max_x);
        self.pan.y = self.pan.y.clamp(-max_y, max_y);
    }

    fn rotated_pixels(&self, image_pixels: [u32; 2]) -> [u32; 2] {
        if self.quarter_turns.is_multiple_of(2) {
            image_pixels
        } else {
            [image_pixels[1], image_pixels[0]]
        }
    }
}

fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

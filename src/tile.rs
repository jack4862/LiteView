use eframe::egui::{Pos2, Rect, Vec2};

pub const TILE_SIZE: u32 = 512;
pub const MAX_TILE_LEVEL: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    pub level: u8,
    pub column: u32,
    pub row: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl SourceRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileGrid {
    image_width: u32,
    image_height: u32,
    level: u8,
    columns: u32,
    rows: u32,
}

impl TileGrid {
    pub fn new(image_pixels: [u32; 2], level: u8) -> Self {
        let level = level.min(MAX_TILE_LEVEL);
        let span = tile_span(level);
        Self {
            image_width: image_pixels[0],
            image_height: image_pixels[1],
            level,
            columns: image_pixels[0].div_ceil(span),
            rows: image_pixels[1].div_ceil(span),
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn columns(&self) -> u32 {
        self.columns
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    pub fn source_rect(&self, coord: TileCoord) -> Option<SourceRect> {
        if coord.level != self.level || coord.column >= self.columns || coord.row >= self.rows {
            return None;
        }
        let span = tile_span(self.level);
        let x = coord.column.saturating_mul(span);
        let y = coord.row.saturating_mul(span);
        Some(SourceRect {
            x,
            y,
            width: span.min(self.image_width.saturating_sub(x)),
            height: span.min(self.image_height.saturating_sub(y)),
        })
    }

    pub fn visible_tiles(&self, visible: SourceRect) -> Vec<TileCoord> {
        if visible.is_empty() || self.columns == 0 || self.rows == 0 {
            return Vec::new();
        }
        let span = tile_span(self.level);
        let first_column = visible.x / span;
        let first_row = visible.y / span;
        let last_column = visible.right().saturating_sub(1) / span;
        let last_row = visible.bottom().saturating_sub(1) / span;
        let mut tiles = Vec::new();

        for row in first_row..=last_row.min(self.rows.saturating_sub(1)) {
            for column in first_column..=last_column.min(self.columns.saturating_sub(1)) {
                tiles.push(TileCoord {
                    level: self.level,
                    column,
                    row,
                });
            }
        }
        tiles
    }

    pub fn output_size(&self, coord: TileCoord) -> [u32; 2] {
        let Some(rect) = self.source_rect(coord) else {
            return [0, 0];
        };
        let span = tile_span(self.level);
        [
            scaled_dimension(rect.width, span),
            scaled_dimension(rect.height, span),
        ]
    }
}

/// Returns the image-source rectangle covered by the rotated viewport.
///
/// `display_size` is the rotated on-screen size used by the viewer mesh.
pub fn visible_source_rect(
    viewport: Rect,
    image_center: Pos2,
    display_size: Vec2,
    rotation_radians: f32,
    image_pixels: [u32; 2],
    margin_pixels: u32,
) -> SourceRect {
    if image_pixels[0] == 0
        || image_pixels[1] == 0
        || display_size.x <= 0.0
        || display_size.y <= 0.0
        || !viewport.is_positive()
    {
        return SourceRect {
            x: 0,
            y: 0,
            width: image_pixels[0],
            height: image_pixels[1],
        };
    }

    let (sin, cos) = (-rotation_radians).sin_cos();
    let corners = [
        viewport.left_top(),
        viewport.right_top(),
        viewport.right_bottom(),
        viewport.left_bottom(),
    ];
    let mut min_u = f32::INFINITY;
    let mut min_v = f32::INFINITY;
    let mut max_u = f32::NEG_INFINITY;
    let mut max_v = f32::NEG_INFINITY;

    for corner in corners {
        let offset = corner - image_center;
        let unrotated = Vec2::new(
            offset.x * cos - offset.y * sin,
            offset.x * sin + offset.y * cos,
        );
        let u = unrotated.x / display_size.x + 0.5;
        let v = unrotated.y / display_size.y + 0.5;
        min_u = min_u.min(u);
        min_v = min_v.min(v);
        max_u = max_u.max(u);
        max_v = max_v.max(v);
    }

    const ROUND_EPSILON: f32 = 0.001;
    let min_x = (min_u.clamp(0.0, 1.0) * image_pixels[0] as f32 + ROUND_EPSILON).floor() as i64;
    let min_y = (min_v.clamp(0.0, 1.0) * image_pixels[1] as f32 + ROUND_EPSILON).floor() as i64;
    let max_x = (max_u.clamp(0.0, 1.0) * image_pixels[0] as f32 - ROUND_EPSILON).ceil() as i64;
    let max_y = (max_v.clamp(0.0, 1.0) * image_pixels[1] as f32 - ROUND_EPSILON).ceil() as i64;
    let margin = i64::from(margin_pixels);
    let x = (min_x - margin).max(0).min(i64::from(image_pixels[0])) as u32;
    let y = (min_y - margin).max(0).min(i64::from(image_pixels[1])) as u32;
    let right = (max_x + margin)
        .max(i64::from(x))
        .min(i64::from(image_pixels[0])) as u32;
    let bottom = (max_y + margin)
        .max(i64::from(y))
        .min(i64::from(image_pixels[1])) as u32;

    SourceRect {
        x,
        y,
        width: right.saturating_sub(x),
        height: bottom.saturating_sub(y),
    }
}

pub fn level_for_zoom(zoom: f32) -> u8 {
    if !zoom.is_finite() || zoom >= 1.0 {
        return 0;
    }
    let level = (1.0 / zoom.max(1.0 / f32::from(1_u16 << MAX_TILE_LEVEL))).log2();
    level.floor().clamp(0.0, f32::from(MAX_TILE_LEVEL)) as u8
}

pub fn tile_span(level: u8) -> u32 {
    TILE_SIZE.saturating_mul(1_u32 << level.min(MAX_TILE_LEVEL))
}

fn scaled_dimension(value: u32, span: u32) -> u32 {
    if value == 0 {
        return 0;
    }
    let scaled = (u64::from(value) * u64::from(TILE_SIZE) + u64::from(span) / 2) / u64::from(span);
    scaled.clamp(1, u64::from(TILE_SIZE)) as u32
}

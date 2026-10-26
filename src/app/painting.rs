use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};

use super::MUTED;

#[derive(Debug, Clone, Copy)]
pub(super) struct TileGeometry {
    pub source_rect: [u32; 4],
    pub image_pixels: [u32; 2],
}

pub(super) fn draw_textured_image(
    painter: &egui::Painter,
    viewport: Rect,
    center: Pos2,
    size: Vec2,
    angle: f32,
    texture_id: egui::TextureId,
) {
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let half = size * 0.5;
    let corners = [
        (Vec2::new(-half.x, -half.y), Pos2::new(0.0, 0.0)),
        (Vec2::new(half.x, -half.y), Pos2::new(1.0, 0.0)),
        (Vec2::new(half.x, half.y), Pos2::new(1.0, 1.0)),
        (Vec2::new(-half.x, half.y), Pos2::new(0.0, 1.0)),
    ];
    let (sin, cos) = angle.sin_cos();
    let mut mesh = egui::epaint::Mesh::with_texture(texture_id);
    for (offset, uv) in corners {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center
                + Vec2::new(
                    offset.x * cos - offset.y * sin,
                    offset.x * sin + offset.y * cos,
                ),
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter
        .with_clip_rect(viewport)
        .add(egui::Shape::mesh(mesh));
}

pub(super) fn draw_textured_tile(
    painter: &egui::Painter,
    viewport: Rect,
    center: Pos2,
    display_size: Vec2,
    angle: f32,
    geometry: TileGeometry,
    texture_id: egui::TextureId,
) {
    if geometry.image_pixels[0] == 0
        || geometry.image_pixels[1] == 0
        || geometry.source_rect[2] == 0
        || geometry.source_rect[3] == 0
        || display_size.x <= 0.0
        || display_size.y <= 0.0
    {
        return;
    }

    let [source_x, source_y, source_width, source_height] = geometry.source_rect;
    let [image_width, image_height] = geometry.image_pixels;
    let u0 = source_x as f32 / image_width as f32;
    let v0 = source_y as f32 / image_height as f32;
    let u1 = (source_x + source_width) as f32 / image_width as f32;
    let v1 = (source_y + source_height) as f32 / image_height as f32;
    let corners = [
        (Vec2::new(u0 - 0.5, v0 - 0.5), Pos2::new(0.0, 0.0)),
        (Vec2::new(u1 - 0.5, v0 - 0.5), Pos2::new(1.0, 0.0)),
        (Vec2::new(u1 - 0.5, v1 - 0.5), Pos2::new(1.0, 1.0)),
        (Vec2::new(u0 - 0.5, v1 - 0.5), Pos2::new(0.0, 1.0)),
    ];
    let (sin, cos) = angle.sin_cos();
    let mut mesh = egui::epaint::Mesh::with_texture(texture_id);
    for (local, uv) in corners {
        let offset = Vec2::new(local.x * display_size.x, local.y * display_size.y);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center
                + Vec2::new(
                    offset.x * cos - offset.y * sin,
                    offset.x * sin + offset.y * cos,
                ),
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter
        .with_clip_rect(viewport)
        .add(egui::Shape::mesh(mesh));
}

pub(super) fn draw_checkerboard(painter: &egui::Painter, viewport: Rect, image_rect: Rect) {
    let visible = viewport.intersect(image_rect);
    if !visible.is_positive() {
        return;
    }
    const CELL: f32 = 16.0;
    let dark = Color32::from_rgb(55, 58, 62);
    let light = Color32::from_rgb(74, 77, 82);
    let min_x = ((visible.min.x - image_rect.min.x) / CELL).floor() as i32;
    let max_x = ((visible.max.x - image_rect.min.x) / CELL).ceil() as i32;
    let min_y = ((visible.min.y - image_rect.min.y) / CELL).floor() as i32;
    let max_y = ((visible.max.y - image_rect.min.y) / CELL).ceil() as i32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let cell = Rect::from_min_size(
                image_rect.min + Vec2::new(x as f32 * CELL, y as f32 * CELL),
                Vec2::splat(CELL),
            )
            .intersect(visible);
            if cell.is_positive() {
                let color = if (x + y) % 2 == 0 { dark } else { light };
                painter.rect_filled(cell, 0.0, color);
            }
        }
    }
}

pub(super) fn draw_empty_state(painter: &egui::Painter, rect: Rect, title: &str, body: &str) {
    let title_galley = painter.layout_no_wrap(
        title.to_owned(),
        FontId::proportional(20.0),
        Color32::from_rgb(226, 229, 233),
    );
    let body_galley = painter.layout(
        body.to_owned(),
        FontId::proportional(14.0),
        MUTED,
        (rect.width() - 64.0).max(200.0),
    );
    let total_height = title_galley.size().y + 10.0 + body_galley.size().y;
    let title_pos = Pos2::new(rect.center().x, rect.center().y - total_height * 0.5);
    painter.galley(
        Pos2::new(title_pos.x - title_galley.size().x * 0.5, title_pos.y),
        title_galley.clone(),
        Color32::WHITE,
    );
    painter.galley(
        Pos2::new(
            rect.center().x - body_galley.size().x * 0.5,
            title_pos.y + title_galley.size().y + 10.0,
        ),
        body_galley,
        MUTED,
    );
}

pub(super) fn draw_loading_pill(painter: &egui::Painter, viewport: Rect, text: &str) {
    let rect = Rect::from_center_size(
        Pos2::new(viewport.center().x, viewport.max.y - 28.0),
        Vec2::new(112.0, 26.0),
    );
    painter.rect_filled(rect, 6.0, Color32::from_black_alpha(210));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        Color32::from_rgb(232, 234, 237),
    );
}

pub(super) fn draw_drop_overlay(painter: &egui::Painter, viewport: Rect, text: &str) {
    let rect = viewport.shrink(18.0);
    painter.rect_filled(rect, 6.0, Color32::from_black_alpha(185));
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(2.0, Color32::from_rgb(116, 171, 235)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::proportional(20.0),
        Color32::WHITE,
    );
}

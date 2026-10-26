use eframe::egui::{Pos2, Rect, Vec2};
use liteview::view::ViewState;

#[test]
fn fit_zoom_accounts_for_image_pixels_and_display_scale() {
    let view = ViewState::default();

    assert!((view.effective_zoom([400, 200], Vec2::splat(200.0), 1.0) - 0.5).abs() < 0.0001);
    assert!((view.effective_zoom([400, 200], Vec2::splat(100.0), 2.0) - 0.5).abs() < 0.0001);
}

#[test]
fn display_rotation_swaps_dimensions_before_fitting() {
    let mut view = ViewState::default();
    view.rotate_clockwise();

    assert!((view.effective_zoom([400, 200], Vec2::splat(100.0), 1.0) - 0.25).abs() < 0.0001);
}

#[test]
fn cursor_anchor_is_preserved_when_zooming() {
    let mut view = ViewState::default();
    let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0));

    view.zoom_at(1.0, Pos2::new(150.0, 100.0), viewport, [400, 200], 1.0);

    assert!((view.pan().x + 50.0).abs() < 0.0001);
    assert!(view.pan().y.abs() < 0.0001);
    assert!((view.effective_zoom([400, 200], viewport.size(), 1.0) - 1.0).abs() < 0.0001);
}

#[test]
fn panning_is_clamped_to_keep_smaller_dimensions_centered() {
    let mut view = ViewState::default();
    view.set_manual_zoom(1.0);
    view.set_pan(Vec2::new(10_000.0, -10_000.0));

    view.clamp_pan([400, 200], Vec2::new(200.0, 300.0), 1.0);

    assert!((view.pan().x - 100.0).abs() < 0.0001);
    assert!(view.pan().y.abs() < 0.0001);
}

#[test]
fn actual_size_and_fit_modes_reset_zoom_state() {
    let mut view = ViewState::default();
    view.set_manual_zoom(3.0);
    view.set_pan(Vec2::splat(50.0));

    view.actual_size();
    assert!((view.effective_zoom([400, 200], Vec2::splat(200.0), 1.0) - 1.0).abs() < 0.0001);
    assert_eq!(view.pan(), Vec2::ZERO);

    view.fit();
    assert!((view.effective_zoom([400, 200], Vec2::splat(200.0), 1.0) - 0.5).abs() < 0.0001);
    assert_eq!(view.pan(), Vec2::ZERO);
}

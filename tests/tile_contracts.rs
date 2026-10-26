use eframe::egui::{Pos2, Rect, Vec2};
use liteview::tile::{SourceRect, TileCoord, TileGrid, level_for_zoom, visible_source_rect};

#[test]
fn builds_clipped_tiles_and_scaled_edge_outputs() {
    let grid = TileGrid::new([1300, 700], 0);

    assert_eq!((grid.columns(), grid.rows()), (3, 2));
    assert_eq!(
        grid.source_rect(TileCoord {
            level: 0,
            column: 2,
            row: 1,
        }),
        Some(SourceRect {
            x: 1024,
            y: 512,
            width: 276,
            height: 188,
        })
    );
    assert_eq!(
        grid.output_size(TileCoord {
            level: 0,
            column: 2,
            row: 1,
        }),
        [276, 188]
    );
}

#[test]
fn selects_only_tiles_intersecting_the_visible_rect() {
    let grid = TileGrid::new([1536, 1024], 0);
    let visible = SourceRect {
        x: 500,
        y: 500,
        width: 600,
        height: 100,
    };

    assert_eq!(
        grid.visible_tiles(visible),
        vec![
            TileCoord {
                level: 0,
                column: 0,
                row: 0,
            },
            TileCoord {
                level: 0,
                column: 1,
                row: 0,
            },
            TileCoord {
                level: 0,
                column: 2,
                row: 0,
            },
            TileCoord {
                level: 0,
                column: 0,
                row: 1,
            },
            TileCoord {
                level: 0,
                column: 1,
                row: 1,
            },
            TileCoord {
                level: 0,
                column: 2,
                row: 1,
            },
        ]
    );
}

#[test]
fn maps_rotated_viewport_back_to_source_pixels() {
    let viewport = Rect::from_min_max(Pos2::new(50.0, 50.0), Pos2::new(150.0, 150.0));
    let display_size = Vec2::new(200.0, 400.0);
    let visible = visible_source_rect(
        viewport,
        Pos2::new(100.0, 100.0),
        display_size,
        std::f32::consts::FRAC_PI_2,
        [1000, 500],
        0,
    );

    assert_eq!(
        visible,
        SourceRect {
            x: 250,
            y: 187,
            width: 500,
            height: 126,
        }
    );
}

#[test]
fn chooses_lod_levels_and_clamps_tiny_zoom() {
    assert_eq!(level_for_zoom(2.0), 0);
    assert_eq!(level_for_zoom(1.0), 0);
    assert_eq!(level_for_zoom(0.75), 0);
    assert_eq!(level_for_zoom(0.5), 1);
    assert_eq!(level_for_zoom(0.25), 2);
    assert_eq!(level_for_zoom(0.0), 6);
}

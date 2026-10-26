#![cfg(windows)]

use std::{fs, path::Path};

use image::{ImageFormat, Rgb, RgbImage};
use liteview::{
    tile_decoder::JpegTileDecoder,
    tile_service::{TileDecodeRequest, TileDecoder},
};
use tempfile::TempDir;

#[test]
fn decodes_display_rectangles_with_exif_orientation() {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path().join("oriented.jpg");
    write_halves(&path, 64, 32);
    let jpeg = add_exif_orientation(&fs::read(&path).expect("read JPEG"), 6);
    fs::write(&path, jpeg).expect("write oriented JPEG");

    let rgba = JpegTileDecoder
        .decode(&TileDecodeRequest {
            path,
            source_dimensions: [64, 32],
            source_rect: [0, 0, 32, 64],
            output_size: [16, 32],
            exif_orientation: 6,
            icc_profile: None,
        })
        .expect("decode oriented tile");

    assert_eq!(rgba.len(), 16 * 32 * 4);
    let top = average_rgb(&rgba, 16, 0);
    let bottom = average_rgb(&rgba, 16, 16);
    assert!(top[0] > top[2] + 80.0, "top average was {top:?}");
    assert!(
        bottom[2] > bottom[0] + 80.0,
        "bottom average was {bottom:?}"
    );
    assert!(rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
}

#[test]
fn rejects_zero_sized_requests_before_decoding() {
    let error = JpegTileDecoder
        .decode(&TileDecodeRequest {
            path: "missing.jpg".into(),
            source_dimensions: [16, 16],
            source_rect: [0, 0, 0, 16],
            output_size: [8, 8],
            exif_orientation: 1,
            icc_profile: None,
        })
        .expect_err("zero-sized tile must fail");

    assert!(error.contains("zero dimensions"));
}

fn write_halves(path: &Path, width: u32, height: u32) {
    let mut image = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let color = if x < width / 2 {
                Rgb([235, 20, 20])
            } else {
                Rgb([20, 20, 235])
            };
            image.put_pixel(x, y, color);
        }
    }
    image.save_with_format(path, ImageFormat::Jpeg).unwrap();
}

fn add_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    assert!(jpeg.starts_with(&[0xFF, 0xD8]));
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112_u16.to_le_bytes());
    tiff.extend_from_slice(&3_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0_u32.to_le_bytes());

    let payload_length = 2 + 6 + tiff.len();
    let mut app1 = Vec::with_capacity(payload_length);
    app1.extend_from_slice(&[0xFF, 0xE1]);
    app1.extend_from_slice(&(payload_length as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);

    let mut result = Vec::with_capacity(jpeg.len() + app1.len());
    result.extend_from_slice(&jpeg[..2]);
    result.extend_from_slice(&app1);
    result.extend_from_slice(&jpeg[2..]);
    result
}

fn average_rgb(rgba: &[u8], width: usize, start_row: usize) -> [f64; 3] {
    let start = start_row * width * 4;
    let end = start + 16 * width * 4;
    let mut sums = [0_u64; 3];
    for pixel in rgba[start..end].as_chunks::<4>().0 {
        for channel in 0..3 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    let pixels = 16 * width;
    sums.map(|sum| sum as f64 / pixels as f64)
}

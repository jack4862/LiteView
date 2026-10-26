#![cfg(windows)]

#[path = "../src/wic.rs"]
mod wic;

use std::fs;
use std::path::Path;
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use tempfile::TempDir;
use wic::{
    DEFAULT_PREVIEW_LONG_EDGE, SourceRect, WicError, decode_jpeg_preview, decode_jpeg_region,
    decode_jpeg_scaled, decode_jpeg_target,
};

#[test]
fn preview_and_explicit_targets_are_usable() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("target.jpg");
    write_gradient_jpeg(&path, 160, 80);

    assert_eq!(DEFAULT_PREVIEW_LONG_EDGE, 2048);
    let preview = decode_jpeg_preview(&path).unwrap();
    assert_eq!(preview.output_dimensions, [160, 80]);
    assert!(preview.elapsed < Duration::from_secs(30));

    let targeted = decode_jpeg_target(&path, [20, 20]).unwrap();
    assert_eq!(targeted.output_dimensions, [20, 10]);
    assert_eq!(targeted.pixels.len(), 20 * 10 * 4);
}

#[test]
fn generated_jpeg_scales_to_requested_long_edge() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("scaled.jpg");
    write_gradient_jpeg(&path, 160, 80);

    let decoded = decode_jpeg_scaled(&path, 40).unwrap();

    assert_eq!(decoded.source_dimensions, [160, 80]);
    assert_eq!(decoded.display_dimensions, [160, 80]);
    assert_eq!(decoded.output_dimensions, [40, 20]);
    assert_eq!(decoded.orientation, 1);
    assert_eq!(decoded.source_rect, SourceRect::new(0, 0, 160, 80));
    assert_eq!(
        decoded.pixels.len(),
        (decoded.output_dimensions[0] * decoded.output_dimensions[1] * 4) as usize
    );
    assert!(
        decoded
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert!(decoded.icc_profile.is_none());
    assert!(
        decoded
            .pixels
            .iter()
            .any(|value| *value != decoded.pixels[0])
    );
}

#[test]
fn generated_jpeg_region_reads_the_requested_source_rect() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("region.jpg");
    write_two_tone_jpeg(&path, 64, 48);
    let rect = SourceRect::new(32, 0, 16, 16);

    let decoded = decode_jpeg_region(&path, rect, [8, 8]).unwrap();

    assert_eq!(decoded.source_dimensions, [64, 48]);
    assert_eq!(decoded.output_dimensions, [8, 8]);
    assert_eq!(decoded.source_rect, rect);
    assert_eq!(decoded.pixels.len(), 8 * 8 * 4);
    let average = average_rgb(&decoded.pixels);
    assert!(average[2] > 150.0, "blue average was {average:?}");
    assert!(average[0] < 100.0, "red average was {average:?}");
    assert!(
        decoded
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
}

#[test]
fn exif_orientation_is_reported_and_display_dimensions_swap() {
    let temp = TempDir::new().unwrap();
    let baseline = temp.path().join("baseline.jpg");
    write_gradient_jpeg(&baseline, 16, 8);
    let oriented = temp.path().join("oriented.jpg");
    let jpeg = add_exif_orientation(&fs::read(&baseline).unwrap(), 6);
    fs::write(&oriented, jpeg).unwrap();

    let decoded = decode_jpeg_scaled(&oriented, 8).unwrap();

    assert_eq!(decoded.source_dimensions, [16, 8]);
    assert_eq!(decoded.display_dimensions, [8, 16]);
    assert_eq!(decoded.output_dimensions, [8, 4]);
    assert_eq!(decoded.orientation, 6);
}

#[test]
fn zero_targets_and_invalid_rects_are_rejected() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("validation.jpg");
    write_gradient_jpeg(&path, 8, 8);

    assert!(matches!(
        decode_jpeg_scaled(&path, 0),
        Err(WicError::InvalidTargetSize)
    ));
    assert!(matches!(
        decode_jpeg_target(&path, [0, 8]),
        Err(WicError::InvalidTargetSize)
    ));
    assert!(matches!(
        decode_jpeg_region(&path, SourceRect::new(0, 0, 0, 8), [4, 4]),
        Err(WicError::InvalidSourceRect)
    ));
    assert!(matches!(
        decode_jpeg_region(&path, SourceRect::new(4, 4, 5, 5), [4, 4]),
        Err(WicError::SourceRectOutOfBounds { .. })
    ));
}

#[test]
fn non_jpeg_input_is_rejected() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("not-jpeg.png");
    RgbImage::new(8, 8)
        .save_with_format(&path, ImageFormat::Png)
        .unwrap();

    assert!(matches!(
        decode_jpeg_scaled(&path, 4),
        Err(WicError::NotJpeg { .. })
    ));
}

fn write_gradient_jpeg(path: &Path, width: u32, height: u32) {
    let mut image = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let red = ((x * 255) / width.max(1)) as u8;
            let green = ((y * 255) / height.max(1)) as u8;
            image.put_pixel(x, y, Rgb([red, green, 160]));
        }
    }
    image.save_with_format(path, ImageFormat::Jpeg).unwrap();
}

fn write_two_tone_jpeg(path: &Path, width: u32, height: u32) {
    let mut image = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let color = if x < width / 2 {
                Rgb([240, 20, 20])
            } else {
                Rgb([20, 20, 240])
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
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes());

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

fn average_rgb(pixels: &[u8]) -> [f64; 3] {
    let count = pixels.len() / 4;
    let mut sums = [0u64; 3];
    for pixel in pixels.as_chunks::<4>().0 {
        for channel in 0..3 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    sums.map(|sum| sum as f64 / count as f64)
}

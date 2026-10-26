use std::fs;
use std::panic::{self, AssertUnwindSafe};

use image::{
    ExtendedColorType, GrayImage, ImageEncoder, Luma, Rgb, RgbImage, Rgba, RgbaImage,
    codecs::{jpeg::JpegEncoder, webp::WebPEncoder},
};
use liteview::{
    loader::{IccStatus, LoadError, LoadOptions, SourceFormat, load},
    navigation::DirectorySnapshot,
};
use tempfile::tempdir;

fn write_png(path: &std::path::Path, width: u32, height: u32) {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x.saturating_mul(17)) as u8,
            (y.saturating_mul(29)) as u8,
            96,
            if x == 0 && y == 0 { 128 } else { 255 },
        ])
    });
    image
        .save_with_format(path, image::ImageFormat::Png)
        .expect("write PNG fixture");
}

fn write_webp(path: &std::path::Path) {
    let image = RgbaImage::from_raw(2, 1, vec![255, 0, 0, 32, 0, 255, 0, 224])
        .expect("valid WebP pixel buffer");
    let mut encoded = Vec::new();
    WebPEncoder::new_lossless(&mut encoded)
        .encode(image.as_raw(), 2, 1, ExtendedColorType::Rgba8)
        .expect("encode WebP fixture");
    fs::write(path, encoded).expect("write WebP fixture");
}

fn write_jpeg(
    path: &std::path::Path,
    width: u32,
    height: u32,
    orientation: Option<u16>,
    icc_profile: Option<&[u8]>,
) {
    let image = RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x * 31) as u8, (y * 47) as u8, 128])
    });
    let mut encoded = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut encoded, 100);

    if let Some(orientation) = orientation {
        encoder
            .set_exif_metadata(tiff_orientation(orientation))
            .expect("set EXIF orientation");
    }
    if let Some(icc_profile) = icc_profile {
        encoder
            .set_icc_profile(icc_profile.to_vec())
            .expect("set ICC profile");
    }

    encoder
        .write_image(&image, width, height, ExtendedColorType::Rgb8)
        .expect("encode JPEG fixture");
    fs::write(path, encoded).expect("write JPEG fixture");
}

fn tiff_orientation(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42_u16.to_le_bytes());
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112_u16.to_le_bytes());
    tiff.extend_from_slice(&3_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&0_u16.to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    tiff
}

#[test]
fn filters_supported_files_and_sorts_naturally() {
    let directory = tempdir().expect("temporary directory");
    for name in [
        "photo10.JPG",
        "photo2.png",
        "photo1.webp",
        "notes.txt",
        "cover.jpeg",
    ] {
        fs::write(directory.path().join(name), []).expect("create file");
    }

    let selected = directory.path().join("photo2.png");
    let snapshot = DirectorySnapshot::scan(&selected).expect("scan directory");
    let names: Vec<_> = snapshot
        .files()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    assert_eq!(
        names,
        ["cover.jpeg", "photo1.webp", "photo2.png", "photo10.JPG"]
    );
    assert_eq!(snapshot.selected_index(), 2);
    assert_eq!(snapshot.current(), Some(selected.as_path()));
    assert_eq!(
        snapshot.previous().and_then(std::path::Path::to_str),
        Some(directory.path().join("photo1.webp").to_str().unwrap())
    );
    assert_eq!(
        snapshot.next().and_then(std::path::Path::to_str),
        Some(directory.path().join("photo10.JPG").to_str().unwrap())
    );
}

#[test]
fn loads_png_as_unmultiplied_rgba() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("sample.png");
    write_png(&path, 3, 2);

    let loaded = load(&path, &LoadOptions::default()).expect("load PNG");

    assert_eq!(loaded.format, SourceFormat::Png);
    assert_eq!(loaded.source_dimensions, [3, 2]);
    assert_eq!(loaded.display_dimensions, [3, 2]);
    assert_eq!(loaded.preview_dimensions, [3, 2]);
    assert_eq!(loaded.rgba.len(), 3 * 2 * 4);
    assert_eq!(&loaded.rgba[..4], &[0, 0, 96, 128]);
    assert_eq!(loaded.icc_status, IccStatus::Absent);
    assert!(!loaded.downsampled);
}

#[test]
fn loads_lossless_webp_with_transparency_and_preserves_gray_png() {
    let directory = tempdir().expect("temporary directory");
    let webp_path = directory.path().join("transparent.webp");
    let gray_path = directory.path().join("gray.png");
    write_webp(&webp_path);
    GrayImage::from_pixel(2, 2, Luma([73]))
        .save(&gray_path)
        .expect("write grayscale PNG fixture");

    let webp = load(&webp_path, &LoadOptions::default()).expect("load WebP");
    let gray = load(&gray_path, &LoadOptions::default()).expect("load gray PNG");

    assert_eq!(webp.format, SourceFormat::WebP);
    assert_eq!(webp.preview_dimensions, [2, 1]);
    assert_eq!(&webp.rgba[3..4], &[32]);
    assert_eq!(&webp.rgba[7..8], &[224]);
    assert_eq!(gray.format, SourceFormat::Png);
    assert_eq!(&gray.rgba[..4], &[73, 73, 73, 255]);
}

#[test]
fn detects_image_format_from_content() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("misleading.jpg");
    write_png(&path, 2, 2);

    let loaded = load(&path, &LoadOptions::default()).expect("sniff PNG content");

    assert_eq!(loaded.format, SourceFormat::Png);
}

#[test]
fn applies_exif_orientation_without_changing_source_dimensions() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("rotated.jpg");
    write_jpeg(&path, 3, 2, Some(6), None);

    let loaded = load(&path, &LoadOptions::default()).expect("load oriented JPEG");

    assert_eq!(loaded.source_dimensions, [3, 2]);
    assert_eq!(loaded.display_dimensions, [2, 3]);
    assert_eq!(loaded.preview_dimensions, [2, 3]);
}

#[test]
fn caps_large_previews_while_retaining_source_size() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("wide.png");
    write_png(&path, 100, 40);
    let options = LoadOptions {
        max_preview_side: 20,
        ..LoadOptions::default()
    };

    let loaded = load(&path, &options).expect("load capped preview");

    assert_eq!(loaded.source_dimensions, [100, 40]);
    assert_eq!(loaded.preview_dimensions, [20, 8]);
    assert!(loaded.downsampled);
    assert_eq!(loaded.rgba.len(), 20 * 8 * 4);
}

#[cfg(windows)]
#[test]
fn caps_wic_preview_after_codec_scaling() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("very-wide.jpg");
    write_jpeg(&path, 9_000, 10, None, None);
    let options = LoadOptions {
        max_preview_side: 2048,
        ..LoadOptions::default()
    };

    let loaded = load(&path, &options).expect("load capped WIC preview");

    assert_eq!(loaded.source_dimensions, [9000, 10]);
    assert!(loaded.preview_dimensions[0] <= 2048);
    assert!(loaded.downsampled);
    assert_eq!(
        loaded.rgba.len(),
        loaded.preview_dimensions[0] as usize * loaded.preview_dimensions[1] as usize * 4
    );
}

#[test]
fn keeps_pixels_when_embedded_icc_profile_is_invalid() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("bad-profile.jpg");
    write_jpeg(&path, 4, 3, None, Some(b"not an ICC profile"));

    let loaded = load(&path, &LoadOptions::default()).expect("load with ICC fallback");

    assert_eq!(loaded.icc_status, IccStatus::Invalid);
    assert_eq!(loaded.preview_dimensions, [4, 3]);
}

#[test]
fn rejects_missing_and_corrupt_files() {
    let directory = tempdir().expect("temporary directory");
    let missing = directory.path().join("missing.png");
    let corrupt = directory.path().join("corrupt.png");
    fs::write(&corrupt, b"not a PNG").expect("write corrupt fixture");

    assert!(matches!(
        load(&missing, &LoadOptions::default()),
        Err(LoadError::Open { .. })
    ));
    assert!(matches!(
        load(&corrupt, &LoadOptions::default()),
        Err(LoadError::Decode { .. })
    ));
}

#[test]
fn enforces_pixel_budget_before_allocation() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("too-many-pixels.png");
    write_png(&path, 4, 4);
    let options = LoadOptions {
        max_pixels: 15,
        ..LoadOptions::default()
    };

    assert!(matches!(
        load(&path, &options),
        Err(LoadError::TooLarge { .. })
    ));
}

#[test]
fn handles_edge_navigation_without_wrapping() {
    let directory = tempdir().expect("temporary directory");
    for name in ["first.png", "second.png"] {
        fs::write(directory.path().join(name), []).expect("create file");
    }

    let first = DirectorySnapshot::scan(directory.path().join("first.png")).expect("scan");
    let second = DirectorySnapshot::scan(directory.path().join("second.png")).expect("scan");

    assert_eq!(first.previous(), None);
    assert!(first.next().is_some());
    assert!(second.previous().is_some());
    assert_eq!(second.next(), None);
}

#[test]
fn truncated_and_mutated_supported_inputs_return_errors_without_panicking() {
    let directory = tempdir().expect("temporary directory");
    let png = directory.path().join("sample.png");
    let jpeg = directory.path().join("sample.jpg");
    let webp = directory.path().join("sample.webp");
    write_png(&png, 4, 3);
    write_jpeg(&jpeg, 4, 3, None, None);
    write_webp(&webp);
    let originals = [png, jpeg, webp];

    for original in &originals {
        let bytes = fs::read(original).expect("read valid fixture");
        for &portion in &[0, bytes.len() / 4, bytes.len() / 2, bytes.len() * 3 / 4] {
            let truncated = original.with_extension("truncated.tmp");
            fs::write(&truncated, &bytes[..portion]).expect("write truncated fixture");
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                load(&truncated, &LoadOptions::default())
            }))
            .expect("truncated decode must not panic");
            assert!(result.is_err(), "truncation to {portion} should fail");

            for &offset in &[0, bytes.len() / 2, bytes.len() - 1] {
                let mut mutated = bytes.clone();
                mutated[offset] = mutated[offset].wrapping_add(1);
                let mutated_path = original.with_extension("mutated.tmp");
                fs::write(&mutated_path, mutated).expect("write mutated fixture");
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    load(&mutated_path, &LoadOptions::default())
                }))
                .expect("mutated decode must not panic");
                let _ = result;
            }
        }
    }
}

#[test]
fn loads_from_paths_with_spaces_and_unicode() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("图片 目录").join("测试 图像.png");
    fs::create_dir_all(path.parent().unwrap()).expect("create Unicode directory");
    write_png(&path, 2, 2);

    let loaded = load(&path, &LoadOptions::default()).expect("load from Unicode path");
    assert_eq!(loaded.preview_dimensions, [2, 2]);
}

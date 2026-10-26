use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, metadata::Orientation};
use qcms::{DataType, Intent, Profile, Transform};
use thiserror::Error;

use crate::{
    metadata::{self, MetadataSummary},
    orientation,
};

const MAX_DECODE_ALLOCATION: u64 = 512 * 1024 * 1024;
const WIC_FALLBACK_PIXELS: u64 = 32_000_000;

#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub max_preview_side: u32,
    pub max_pixels: u64,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            max_preview_side: 4096,
            max_pixels: 150_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Jpeg,
    Png,
    WebP,
}

impl SourceFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::WebP => "WebP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IccStatus {
    Absent,
    Converted,
    Invalid,
}

#[derive(Debug, Clone)]
enum IccProbe {
    Absent,
    Profile(Vec<u8>),
    Invalid,
}

impl IccProbe {
    fn from_result(profile: Result<Option<Vec<u8>>, image::ImageError>) -> Self {
        match profile {
            Ok(Some(profile)) => Self::Profile(profile),
            Ok(None) => Self::Absent,
            Err(_) => Self::Invalid,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub path: PathBuf,
    pub format: SourceFormat,
    pub exif_orientation: u8,
    pub source_dimensions: [u32; 2],
    pub display_dimensions: [u32; 2],
    pub preview_dimensions: [u32; 2],
    pub rgba: Vec<u8>,
    pub icc_profile: Option<Vec<u8>>,
    pub file_size: u64,
    pub icc_status: IccStatus,
    pub downsampled: bool,
    pub elapsed: Duration,
    pub metadata: MetadataSummary,
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("cannot open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unsupported image format: {0}")]
    UnsupportedFormat(String),
    #[error("image is too large: {width}x{height} ({pixels} pixels, limit {limit})")]
    TooLarge {
        width: u32,
        height: u32,
        pixels: u64,
        limit: u64,
    },
    #[error("cannot decode {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
    #[error("cannot decode {path}: {message}")]
    Decoder { path: PathBuf, message: String },
}

pub fn load(path: impl AsRef<Path>, options: &LoadOptions) -> Result<LoadedImage, LoadError> {
    let started = Instant::now();
    let path = path.as_ref();
    let file_size = fs::metadata(path)
        .map_err(|source| open_error(path, source))?
        .len();
    let mut reader = ImageReader::open(path)
        .map_err(|source| open_error(path, source))?
        .with_guessed_format()
        .map_err(|source| decode_error(path, image::ImageError::IoError(source)))?;
    let format = reader
        .format()
        .and_then(source_format)
        .ok_or_else(|| LoadError::UnsupportedFormat(path.display().to_string()))?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|source| decode_error(path, source))?;
    let (source_width, source_height) = decoder.dimensions();
    let source_dimensions = [source_width, source_height];
    let pixels = u64::from(source_dimensions[0]) * u64::from(source_dimensions[1]);
    if pixels > options.max_pixels.max(1) {
        return Err(LoadError::TooLarge {
            width: source_dimensions[0],
            height: source_dimensions[1],
            pixels,
            limit: options.max_pixels.max(1),
        });
    }

    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let exif_orientation = orientation.to_exif();
    let icc_profile = IccProbe::from_result(decoder.icc_profile());

    #[cfg(windows)]
    if format == SourceFormat::Jpeg {
        match load_with_wic(
            path,
            options,
            file_size,
            started,
            source_dimensions,
            exif_orientation,
            icc_profile.clone(),
        ) {
            Ok(image) => return Ok(image),
            Err(error) if pixels > WIC_FALLBACK_PIXELS => {
                return Err(LoadError::Decoder {
                    path: path.to_path_buf(),
                    message: error,
                });
            }
            Err(_) => {}
        }
    }

    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|source| decode_error(path, source))?;
    image.apply_orientation(orientation);

    let display_dimensions = [image.width(), image.height()];
    let preview_dimensions = capped_dimensions(
        display_dimensions[0],
        display_dimensions[1],
        options.max_preview_side.max(1),
    );
    let downsampled = preview_dimensions != display_dimensions;
    if downsampled {
        image = image.resize_exact(
            preview_dimensions[0],
            preview_dimensions[1],
            image::imageops::FilterType::Triangle,
        );
    }
    let mut rgba = image.into_rgba8().into_raw();
    let (icc_profile, icc_status) = apply_icc_profile(&mut rgba, icc_profile);

    Ok(LoadedImage {
        path: path.to_path_buf(),
        format,
        exif_orientation,
        source_dimensions,
        display_dimensions,
        preview_dimensions,
        rgba,
        icc_profile,
        file_size,
        icc_status,
        downsampled,
        elapsed: started.elapsed(),
        metadata: metadata::read_summary(path),
    })
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn load_with_wic(
    path: &Path,
    options: &LoadOptions,
    file_size: u64,
    started: Instant,
    source_dimensions: [u32; 2],
    exif_orientation: u8,
    icc_profile: IccProbe,
) -> Result<LoadedImage, String> {
    use crate::wic;

    let preview_target = wic_preview_long_edge(source_dimensions, options.max_preview_side.max(1));
    let decoded =
        wic::decode_jpeg_scaled(path, preview_target).map_err(|error| error.to_string())?;
    if decoded.source_dimensions != source_dimensions {
        return Err(format!(
            "WIC reported {}x{} but the image header reported {}x{}",
            decoded.source_dimensions[0],
            decoded.source_dimensions[1],
            source_dimensions[0],
            source_dimensions[1]
        ));
    }

    let (mut rgba, oriented_dimensions) = orientation::apply_orientation(
        &decoded.pixels,
        decoded.output_dimensions,
        exif_orientation,
    )
    .map_err(|error| error.to_string())?;
    let preview_dimensions = capped_dimensions(
        oriented_dimensions[0],
        oriented_dimensions[1],
        options.max_preview_side.max(1),
    );
    if preview_dimensions != oriented_dimensions {
        rgba = resize_opaque_rgba_area(&rgba, oriented_dimensions, preview_dimensions)?;
    }
    let (icc_profile, icc_status) = apply_icc_profile(&mut rgba, icc_profile);
    let downsampled = preview_dimensions != decoded.display_dimensions;

    Ok(LoadedImage {
        path: path.to_path_buf(),
        format: SourceFormat::Jpeg,
        exif_orientation,
        source_dimensions,
        display_dimensions: decoded.display_dimensions,
        preview_dimensions,
        rgba,
        icc_profile,
        file_size,
        icc_status,
        downsampled,
        elapsed: started.elapsed(),
        metadata: metadata::read_summary(path),
    })
}

#[cfg(windows)]
fn wic_preview_long_edge(source_dimensions: [u32; 2], max_long_edge: u32) -> u32 {
    let mut dimensions = source_dimensions;
    while dimensions[0] > max_long_edge || dimensions[1] > max_long_edge {
        dimensions[0] = dimensions[0].div_ceil(2).max(1);
        dimensions[1] = dimensions[1].div_ceil(2).max(1);
    }
    dimensions[0].max(dimensions[1])
}

#[cfg(windows)]
fn resize_opaque_rgba_area(
    rgba: &[u8],
    source_dimensions: [u32; 2],
    target_dimensions: [u32; 2],
) -> Result<Vec<u8>, String> {
    let [source_width, source_height] = source_dimensions;
    let [target_width, target_height] = target_dimensions;
    let source_bytes = (source_width as usize)
        .checked_mul(source_height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "source preview dimensions overflowed".to_owned())?;
    if rgba.len() != source_bytes {
        return Err(format!(
            "source preview has {} bytes; expected {source_bytes}",
            rgba.len()
        ));
    }

    let x_weights = area_weights(source_width, target_width);
    let y_weights = area_weights(source_height, target_height);
    let horizontal_bytes = (target_width as usize)
        .checked_mul(source_height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "horizontal preview dimensions overflowed".to_owned())?;
    let mut horizontal = vec![0_u8; horizontal_bytes];

    for source_y in 0..source_height as usize {
        let source_row = source_y * source_width as usize * 4;
        for (target_x, weights) in x_weights.iter().enumerate() {
            let mut channels = [0.0_f32; 4];
            for &(source_x, weight) in weights {
                let source = source_row + source_x * 4;
                for channel in 0..4 {
                    channels[channel] += f32::from(rgba[source + channel]) * weight;
                }
            }
            let destination = (source_y * target_width as usize + target_x) * 4;
            for channel in 0..4 {
                horizontal[destination + channel] =
                    channels[channel].round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    let target_bytes = (target_width as usize)
        .checked_mul(target_height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "target preview dimensions overflowed".to_owned())?;
    let mut target = vec![0_u8; target_bytes];
    for (target_y, weights) in y_weights.iter().enumerate() {
        for target_x in 0..target_width as usize {
            let mut channels = [0.0_f32; 4];
            for &(source_y, weight) in weights {
                let source = (source_y * target_width as usize + target_x) * 4;
                for channel in 0..4 {
                    channels[channel] += f32::from(horizontal[source + channel]) * weight;
                }
            }
            let destination = (target_y * target_width as usize + target_x) * 4;
            for channel in 0..4 {
                target[destination + channel] = channels[channel].round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    Ok(target)
}

#[cfg(windows)]
fn area_weights(source: u32, target: u32) -> Vec<Vec<(usize, f32)>> {
    let scale = f64::from(source) / f64::from(target);
    (0..target)
        .map(|target_index| {
            let start = f64::from(target_index) * scale;
            let end = f64::from(target_index + 1) * scale;
            let first = start.floor() as u32;
            let last = (end.ceil() as u32).min(source);
            (first..last)
                .map(|source_index| {
                    let overlap =
                        end.min(f64::from(source_index + 1)) - start.max(f64::from(source_index));
                    (source_index as usize, (overlap / scale) as f32)
                })
                .collect()
        })
        .collect()
}

fn source_format(format: ImageFormat) -> Option<SourceFormat> {
    match format {
        ImageFormat::Jpeg => Some(SourceFormat::Jpeg),
        ImageFormat::Png => Some(SourceFormat::Png),
        ImageFormat::WebP => Some(SourceFormat::WebP),
        _ => None,
    }
}

pub fn convert_to_srgb(rgba: &mut [u8], icc_profile: Option<&[u8]>) -> IccStatus {
    let Some(icc_profile) = icc_profile else {
        return IccStatus::Absent;
    };
    let Some(source) = Profile::new_from_slice(icc_profile, false) else {
        return IccStatus::Invalid;
    };
    let destination = Profile::new_sRGB();
    let Some(transform) =
        Transform::new(&source, &destination, DataType::RGBA8, Intent::Perceptual)
    else {
        return IccStatus::Invalid;
    };

    transform.apply(rgba);
    IccStatus::Converted
}

fn apply_icc_profile(rgba: &mut [u8], profile: IccProbe) -> (Option<Vec<u8>>, IccStatus) {
    match profile {
        IccProbe::Profile(profile) => {
            let status = convert_to_srgb(rgba, Some(&profile));
            if status == IccStatus::Invalid {
                (None, status)
            } else {
                (Some(profile), status)
            }
        }
        IccProbe::Absent => {
            convert_to_srgb(rgba, None);
            (None, IccStatus::Absent)
        }
        IccProbe::Invalid => (None, IccStatus::Invalid),
    }
}

pub fn capped_dimensions(width: u32, height: u32, max_side: u32) -> [u32; 2] {
    let longest = width.max(height);
    if longest == 0 || longest <= max_side {
        return [width, height];
    }

    let scale = f64::from(max_side) / f64::from(longest);
    [
        ((f64::from(width) * scale).round() as u32).max(1),
        ((f64::from(height) * scale).round() as u32).max(1),
    ]
}

fn open_error(path: &Path, source: std::io::Error) -> LoadError {
    LoadError::Open {
        path: path.to_path_buf(),
        source,
    }
}

fn decode_error(path: &Path, source: image::ImageError) -> LoadError {
    LoadError::Decode {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::capped_dimensions;
    #[cfg(windows)]
    use super::wic_preview_long_edge;

    #[test]
    fn caps_portrait_and_landscape_dimensions() {
        assert_eq!(capped_dimensions(800, 1200, 600), [400, 600]);
        assert_eq!(capped_dimensions(1200, 600, 300), [300, 150]);
        assert_eq!(capped_dimensions(20, 10, 40), [20, 10]);
    }

    #[cfg(windows)]
    #[test]
    fn selects_the_largest_wic_power_of_two_preview_within_the_cap() {
        assert_eq!(wic_preview_long_edge([5476, 3650], 4096), 2738);
        assert_eq!(wic_preview_long_edge([12000, 9000], 4096), 3000);
        assert_eq!(wic_preview_long_edge([9000, 10], 2048), 1125);
    }
}

//! Research decoder for fast, scaled JPEG previews through Windows Imaging Component.
//!
//! WIC's built-in JPEG codec implements [`IWICBitmapSourceTransform`] with native
//! downscaling by factors of 8, 4, or 2. It supports `Rotate0` only, so this module
//! returns source-oriented RGBA pixels and exposes EXIF orientation separately.
//! Full previews request a 2048-pixel long edge by default. Region reads accept a
//! source-pixel rectangle and a target bounding box, then map the rectangle into
//! WIC's scaled coordinate space. The returned `output_dimensions` are authoritative;
//! JPEG scaling may select a different supported size than requested.
//!
//! The module is intentionally not wired into `lib.rs`; tests include it directly so
//! the research code can be evaluated without changing the application loader.

use std::path::Path;
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::Win32::Foundation::{GENERIC_READ, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat8bppGray,
    GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat24bppRGB, GUID_WICPixelFormat32bppBGR,
    GUID_WICPixelFormat32bppBGRA, GUID_WICPixelFormat32bppPBGRA, GUID_WICPixelFormat32bppPRGBA,
    GUID_WICPixelFormat32bppRGBA, IWICBitmapFrameDecode, IWICBitmapSourceTransform,
    IWICImagingFactory, WICBitmapTransformRotate0, WICColorContextProfile,
    WICDecodeMetadataCacheOnLoad, WICRect,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::{VT_I2, VT_I4, VT_UI2, VT_UI4};
use windows::core::w;
use windows::core::{GUID, HSTRING, Interface};

pub const DEFAULT_PREVIEW_LONG_EDGE: u32 = 2048;

const MAX_OUTPUT_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_ICC_BYTES: usize = 16 * 1024 * 1024;
const INITIAL_ICC_BUFFER: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum WicError {
    #[error("target dimensions must both be non-zero")]
    InvalidTargetSize,
    #[error("source rectangle must have non-zero width and height")]
    InvalidSourceRect,
    #[error(
        "source rectangle ({x}, {y}, {width}, {height}) exceeds source dimensions {source_width}x{source_height}"
    )]
    SourceRectOutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        source_width: u32,
        source_height: u32,
    },
    #[error("image reports zero source dimensions")]
    InvalidSourceSize,
    #[error("the selected image is not JPEG (container {container})")]
    NotJpeg { container: String },
    #[error("the JPEG codec does not expose IWICBitmapSourceTransform")]
    TransformUnavailable,
    #[error("the WIC JPEG codec returned invalid scaled dimensions {width}x{height}")]
    InvalidClosestSize { width: u32, height: u32 },
    #[error("requested output is too large ({pixels} pixels)")]
    OutputTooLarge { pixels: u64 },
    #[error("unsupported WIC output pixel format {format}")]
    UnsupportedPixelFormat { format: String },
    #[error("pixel buffer calculation overflowed")]
    PixelBufferOverflow,
    #[error("WIC produced an invalid {actual}-byte buffer; expected {expected} bytes")]
    InvalidPixelBuffer { expected: usize, actual: usize },
    #[error("{operation} failed with HRESULT 0x{code:08X}: {message}")]
    Wic {
        operation: &'static str,
        code: i32,
        message: String,
    },
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
}

#[derive(Debug, Clone)]
pub struct WicJpegImage {
    /// Tightly packed RGBA8 pixels in JPEG source orientation.
    pub pixels: Vec<u8>,
    pub source_dimensions: [u32; 2],
    /// Full image dimensions after applying EXIF orientation (which may swap axes).
    pub display_dimensions: [u32; 2],
    /// Actual dimensions of `pixels`, before applying EXIF orientation.
    pub output_dimensions: [u32; 2],
    /// Requested source-pixel rectangle, or the full image for preview calls.
    pub source_rect: SourceRect,
    /// Normalized EXIF orientation in the range 1..=8.
    pub orientation: u16,
    /// Raw ICC profile bytes, or `None` when WIC exposes no readable profile.
    pub icc_profile: Option<Vec<u8>>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Copy)]
enum FitRequest {
    LongEdge(u32),
    BoundingBox([u32; 2]),
}

#[derive(Debug, Clone, Copy)]
enum PixelLayout {
    Rgba,
    Bgra,
    PremultipliedBgra,
    PremultipliedRgba,
    Bgr24,
    Rgb24,
    Bgrx32,
    Gray8,
}

impl PixelLayout {
    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba
            | Self::Bgra
            | Self::PremultipliedBgra
            | Self::PremultipliedRgba
            | Self::Bgrx32 => 4,
            Self::Bgr24 | Self::Rgb24 => 3,
            Self::Gray8 => 1,
        }
    }
}

/// Decode a full JPEG with a 2048-pixel long edge target.
pub fn decode_jpeg_preview(path: impl AsRef<Path>) -> Result<WicJpegImage, WicError> {
    decode_jpeg_scaled(path, DEFAULT_PREVIEW_LONG_EDGE)
}

/// Decode a full JPEG, requesting a long edge; WIC may return its closest size.
pub fn decode_jpeg_scaled(
    path: impl AsRef<Path>,
    max_long_edge: u32,
) -> Result<WicJpegImage, WicError> {
    decode_internal(path.as_ref(), None, FitRequest::LongEdge(max_long_edge))
}

/// Decode a full JPEG into a target bounding box while preserving aspect ratio.
pub fn decode_jpeg_target(
    path: impl AsRef<Path>,
    target: [u32; 2],
) -> Result<WicJpegImage, WicError> {
    decode_internal(path.as_ref(), None, FitRequest::BoundingBox(target))
}

/// Decode a source-pixel rectangle, fitted into `target` while preserving aspect ratio.
///
/// WIC scales before clipping, so the rectangle is mapped into the codec's actual
/// scaled coordinate space. `source_rect` is always in unrotated JPEG pixel coordinates.
pub fn decode_jpeg_region(
    path: impl AsRef<Path>,
    source_rect: SourceRect,
    target: [u32; 2],
) -> Result<WicJpegImage, WicError> {
    decode_internal(
        path.as_ref(),
        Some(source_rect),
        FitRequest::BoundingBox(target),
    )
}

fn decode_internal(
    path: &Path,
    requested_rect: Option<SourceRect>,
    fit: FitRequest,
) -> Result<WicJpegImage, WicError> {
    validate_fit_request(fit)?;
    let started = Instant::now();
    let _com = ComScope::new()?;

    let factory: IWICImagingFactory = unsafe {
        CoCreateInstance(
            &CLSID_WICImagingFactory,
            None::<&windows::core::IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
    }
    .map_err(|error| windows_error("CoCreateInstance(WICImagingFactory)", error))?;

    let path_wide = HSTRING::from(path.as_os_str());
    let decoder = unsafe {
        factory.CreateDecoderFromFilename(
            &path_wide,
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnLoad,
        )
    }
    .map_err(|error| windows_error("IWICImagingFactory::CreateDecoderFromFilename", error))?;

    let container = unsafe { decoder.GetContainerFormat() }
        .map_err(|error| windows_error("IWICBitmapDecoder::GetContainerFormat", error))?;
    if container != GUID_ContainerFormatJpeg {
        return Err(WicError::NotJpeg {
            container: format!("{container:?}"),
        });
    }

    let frame = unsafe { decoder.GetFrame(0) }
        .map_err(|error| windows_error("IWICBitmapDecoder::GetFrame", error))?;
    let (source_width, source_height) = get_size(&frame)?;
    if source_width == 0 || source_height == 0 {
        return Err(WicError::InvalidSourceSize);
    }
    let source_dimensions = [source_width, source_height];
    let source_rect = requested_rect.unwrap_or(SourceRect::new(0, 0, source_width, source_height));
    validate_source_rect(source_rect, source_dimensions)?;

    let orientation = query_orientation(&frame).unwrap_or(1);
    let display_dimensions = oriented_dimensions(source_dimensions, orientation);
    let icc_profile = read_icc_profile(&frame);

    let target_box = fit_target_box(source_rect, fit)?;
    let desired_full_size = scale_full_size(
        source_dimensions,
        target_box,
        [source_rect.width, source_rect.height],
    )?;

    let transform: IWICBitmapSourceTransform =
        frame.cast().map_err(|_| WicError::TransformUnavailable)?;
    let actual_full_size = closest_size(&transform, desired_full_size)?;
    let scaled_rect = map_source_rect(source_dimensions, actual_full_size, source_rect)?;
    let output_width = scaled_rect.Width as u32;
    let output_height = scaled_rect.Height as u32;
    validate_output_pixels(output_width, output_height)?;

    let mut pixel_format = GUID_WICPixelFormat32bppRGBA;
    unsafe { transform.GetClosestPixelFormat(&mut pixel_format) }.map_err(|error| {
        windows_error("IWICBitmapSourceTransform::GetClosestPixelFormat", error)
    })?;
    let layout = pixel_layout(pixel_format).ok_or_else(|| WicError::UnsupportedPixelFormat {
        format: format!("{pixel_format:?}"),
    })?;

    let row_bytes = (output_width as usize)
        .checked_mul(layout.bytes_per_pixel())
        .ok_or(WicError::PixelBufferOverflow)?;
    let stride = row_bytes
        .checked_add(3)
        .ok_or(WicError::PixelBufferOverflow)?
        & !3;
    let buffer_len = stride
        .checked_mul(output_height as usize)
        .ok_or(WicError::PixelBufferOverflow)?;
    let stride_u32 = u32::try_from(stride).map_err(|_| WicError::PixelBufferOverflow)?;
    let mut raw_pixels = vec![0u8; buffer_len];

    unsafe {
        transform.CopyPixels(
            &scaled_rect,
            actual_full_size[0],
            actual_full_size[1],
            &pixel_format,
            WICBitmapTransformRotate0,
            stride_u32,
            &mut raw_pixels,
        )
    }
    .map_err(|error| windows_error("IWICBitmapSourceTransform::CopyPixels", error))?;

    if raw_pixels.len() != buffer_len {
        return Err(WicError::InvalidPixelBuffer {
            expected: buffer_len,
            actual: raw_pixels.len(),
        });
    }
    let pixels = convert_to_rgba(
        &raw_pixels,
        stride,
        output_width as usize,
        output_height as usize,
        layout,
    )?;

    Ok(WicJpegImage {
        pixels,
        source_dimensions,
        display_dimensions,
        output_dimensions: [output_width, output_height],
        source_rect,
        orientation,
        icc_profile,
        elapsed: started.elapsed(),
    })
}

struct ComScope {
    initialized: bool,
}

impl ComScope {
    fn new() -> Result<Self, WicError> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            Ok(Self { initialized: true })
        } else if result == RPC_E_CHANGED_MODE {
            // COM is already initialized on this thread in another apartment.
            Ok(Self { initialized: false })
        } else {
            Err(hresult_error("CoInitializeEx", result))
        }
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { CoUninitialize() };
        }
    }
}

fn windows_error(operation: &'static str, error: windows::core::Error) -> WicError {
    WicError::Wic {
        operation,
        code: error.code().0,
        message: error.message(),
    }
}

fn hresult_error(operation: &'static str, result: windows::core::HRESULT) -> WicError {
    WicError::Wic {
        operation,
        code: result.0,
        message: result.message(),
    }
}

fn get_size(frame: &IWICBitmapFrameDecode) -> Result<(u32, u32), WicError> {
    let mut width = 0;
    let mut height = 0;
    unsafe { frame.GetSize(&mut width, &mut height) }
        .map_err(|error| windows_error("IWICBitmapSource::GetSize", error))?;
    Ok((width, height))
}

fn closest_size(
    transform: &IWICBitmapSourceTransform,
    requested: [u32; 2],
) -> Result<[u32; 2], WicError> {
    let mut width = requested[0];
    let mut height = requested[1];
    unsafe { transform.GetClosestSize(&mut width, &mut height) }
        .map_err(|error| windows_error("IWICBitmapSourceTransform::GetClosestSize", error))?;
    if width == 0 || height == 0 {
        return Err(WicError::InvalidClosestSize { width, height });
    }
    Ok([width, height])
}

fn validate_fit_request(fit: FitRequest) -> Result<(), WicError> {
    match fit {
        FitRequest::LongEdge(0) => Err(WicError::InvalidTargetSize),
        FitRequest::BoundingBox([width, height]) if width == 0 || height == 0 => {
            Err(WicError::InvalidTargetSize)
        }
        _ => Ok(()),
    }
}

fn fit_target_box(rect: SourceRect, fit: FitRequest) -> Result<[u32; 2], WicError> {
    let [target_width, target_height] = match fit {
        FitRequest::LongEdge(value) => [value, value],
        FitRequest::BoundingBox(size) => size,
    };
    validate_fit_request(FitRequest::BoundingBox([target_width, target_height]))?;

    let width_limited = u64::from(target_width) * u64::from(rect.height)
        <= u64::from(target_height) * u64::from(rect.width);
    let fitted = if width_limited {
        [
            target_width.min(rect.width),
            round_ratio(rect.height, target_width, rect.width).min(rect.height),
        ]
    } else {
        [
            round_ratio(rect.width, target_height, rect.height).min(rect.width),
            target_height.min(rect.height),
        ]
    };
    if fitted[0] == 0 || fitted[1] == 0 {
        return Err(WicError::InvalidTargetSize);
    }
    Ok(fitted)
}

fn scale_full_size(
    source: [u32; 2],
    target_box: [u32; 2],
    rect_size: [u32; 2],
) -> Result<[u32; 2], WicError> {
    if rect_size[0] == 0 || rect_size[1] == 0 {
        return Err(WicError::InvalidSourceRect);
    }
    Ok([
        round_ratio(source[0], target_box[0], rect_size[0]).clamp(1, source[0]),
        round_ratio(source[1], target_box[1], rect_size[1]).clamp(1, source[1]),
    ])
}

fn round_ratio(value: u32, numerator: u32, denominator: u32) -> u32 {
    let denominator = u64::from(denominator);
    let result = (u64::from(value) * u64::from(numerator) + denominator / 2) / denominator;
    result.clamp(1, u64::from(u32::MAX)) as u32
}

fn validate_source_rect(rect: SourceRect, source: [u32; 2]) -> Result<(), WicError> {
    if rect.width == 0 || rect.height == 0 {
        return Err(WicError::InvalidSourceRect);
    }
    let right = u64::from(rect.x) + u64::from(rect.width);
    let bottom = u64::from(rect.y) + u64::from(rect.height);
    if right > u64::from(source[0]) || bottom > u64::from(source[1]) {
        return Err(WicError::SourceRectOutOfBounds {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            source_width: source[0],
            source_height: source[1],
        });
    }
    Ok(())
}

fn map_source_rect(
    source: [u32; 2],
    scaled: [u32; 2],
    rect: SourceRect,
) -> Result<WICRect, WicError> {
    let left = u64::from(rect.x) * u64::from(scaled[0]) / u64::from(source[0]);
    let top = u64::from(rect.y) * u64::from(scaled[1]) / u64::from(source[1]);
    let right = ceil_ratio(
        u64::from(rect.x) + u64::from(rect.width),
        scaled[0],
        source[0],
    );
    let bottom = ceil_ratio(
        u64::from(rect.y) + u64::from(rect.height),
        scaled[1],
        source[1],
    );
    let width = right.saturating_sub(left).min(u64::from(scaled[0]));
    let height = bottom.saturating_sub(top).min(u64::from(scaled[1]));
    if width == 0 || height == 0 {
        return Err(WicError::InvalidSourceRect);
    }
    Ok(WICRect {
        X: i32::try_from(left).map_err(|_| WicError::PixelBufferOverflow)?,
        Y: i32::try_from(top).map_err(|_| WicError::PixelBufferOverflow)?,
        Width: i32::try_from(width).map_err(|_| WicError::PixelBufferOverflow)?,
        Height: i32::try_from(height).map_err(|_| WicError::PixelBufferOverflow)?,
    })
}

fn ceil_ratio(value: u64, numerator: u32, denominator: u32) -> u64 {
    let denominator = u64::from(denominator);
    (value * u64::from(numerator)).div_ceil(denominator)
}

fn validate_output_pixels(width: u32, height: u32) -> Result<(), WicError> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_OUTPUT_PIXELS {
        return Err(WicError::OutputTooLarge { pixels });
    }
    Ok(())
}

fn pixel_layout(format: GUID) -> Option<PixelLayout> {
    if format == GUID_WICPixelFormat32bppRGBA {
        Some(PixelLayout::Rgba)
    } else if format == GUID_WICPixelFormat32bppBGRA {
        Some(PixelLayout::Bgra)
    } else if format == GUID_WICPixelFormat32bppPBGRA {
        Some(PixelLayout::PremultipliedBgra)
    } else if format == GUID_WICPixelFormat32bppPRGBA {
        Some(PixelLayout::PremultipliedRgba)
    } else if format == GUID_WICPixelFormat24bppBGR {
        Some(PixelLayout::Bgr24)
    } else if format == GUID_WICPixelFormat24bppRGB {
        Some(PixelLayout::Rgb24)
    } else if format == GUID_WICPixelFormat32bppBGR {
        Some(PixelLayout::Bgrx32)
    } else if format == GUID_WICPixelFormat8bppGray {
        Some(PixelLayout::Gray8)
    } else {
        None
    }
}

fn convert_to_rgba(
    raw: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    layout: PixelLayout,
) -> Result<Vec<u8>, WicError> {
    let pixel_count = width
        .checked_mul(height)
        .ok_or(WicError::PixelBufferOverflow)?;
    let expected_len = pixel_count
        .checked_mul(4)
        .ok_or(WicError::PixelBufferOverflow)?;
    let minimum_stride = width
        .checked_mul(layout.bytes_per_pixel())
        .ok_or(WicError::PixelBufferOverflow)?;
    if stride < minimum_stride
        || raw
            .len()
            .checked_div(stride)
            .is_none_or(|rows| rows < height)
    {
        return Err(WicError::InvalidPixelBuffer {
            expected: stride
                .checked_mul(height)
                .ok_or(WicError::PixelBufferOverflow)?,
            actual: raw.len(),
        });
    }

    let mut rgba = vec![0u8; expected_len];
    for y in 0..height {
        let row = &raw[y * stride..y * stride + minimum_stride];
        let output_row = &mut rgba[y * width * 4..(y + 1) * width * 4];
        match layout {
            PixelLayout::Rgba => output_row.copy_from_slice(row),
            PixelLayout::Bgra | PixelLayout::PremultipliedBgra => {
                for (pixel, target) in row
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(output_row.as_chunks_mut::<4>().0)
                {
                    let alpha = pixel[3];
                    let ([red, green, blue], _) =
                        if matches!(layout, PixelLayout::PremultipliedBgra) {
                            unpremultiply([pixel[2], pixel[1], pixel[0]], alpha)
                        } else {
                            ([pixel[2], pixel[1], pixel[0]], alpha)
                        };
                    target.copy_from_slice(&[red, green, blue, alpha]);
                }
            }
            PixelLayout::PremultipliedRgba => {
                for (pixel, target) in row
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(output_row.as_chunks_mut::<4>().0)
                {
                    let alpha = pixel[3];
                    let (colors, _) = unpremultiply([pixel[0], pixel[1], pixel[2]], alpha);
                    target.copy_from_slice(&[colors[0], colors[1], colors[2], alpha]);
                }
            }
            PixelLayout::Bgr24 => {
                for (pixel, target) in row
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .zip(output_row.as_chunks_mut::<4>().0)
                {
                    target.copy_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
            }
            PixelLayout::Rgb24 => {
                for (pixel, target) in row
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .zip(output_row.as_chunks_mut::<4>().0)
                {
                    target.copy_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
            }
            PixelLayout::Bgrx32 => {
                for (pixel, target) in row
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(output_row.as_chunks_mut::<4>().0)
                {
                    target.copy_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
            }
            PixelLayout::Gray8 => {
                for (gray, target) in row.iter().zip(output_row.as_chunks_mut::<4>().0) {
                    target.copy_from_slice(&[*gray, *gray, *gray, 255]);
                }
            }
        }
    }
    Ok(rgba)
}

fn unpremultiply(colors: [u8; 3], alpha: u8) -> ([u8; 3], u8) {
    if alpha == 0 {
        return ([0, 0, 0], 0);
    }
    let alpha = u16::from(alpha);
    let restore = |value: u8| ((u16::from(value) * 255 + alpha / 2) / alpha).min(255) as u8;
    (
        [restore(colors[0]), restore(colors[1]), restore(colors[2])],
        alpha as u8,
    )
}

fn oriented_dimensions(source: [u32; 2], orientation: u16) -> [u32; 2] {
    if matches!(orientation, 5..=8) {
        [source[1], source[0]]
    } else {
        source
    }
}

fn query_orientation(frame: &IWICBitmapFrameDecode) -> Option<u16> {
    let reader = unsafe { frame.GetMetadataQueryReader() }.ok()?;
    for name in [
        w!("/app1/ifd/{ushort=274}"),
        w!("/app1/ifd/exif/{ushort=274}"),
    ] {
        let mut value = PROPVARIANT::default();
        let result = unsafe { reader.GetMetadataByName(name, &mut value) };
        let orientation = if result.is_ok() {
            orientation_from_propvariant(&value)
        } else {
            None
        };
        let _ = unsafe { PropVariantClear(&mut value) };
        if let Some(orientation) = orientation {
            return Some(normalize_orientation(orientation));
        }
    }
    None
}

fn orientation_from_propvariant(value: &PROPVARIANT) -> Option<u16> {
    unsafe {
        let inner = &value.Anonymous.Anonymous;
        match inner.vt {
            VT_UI2 => Some(inner.Anonymous.uiVal),
            VT_I2 => u16::try_from(inner.Anonymous.iVal).ok(),
            VT_UI4 => u16::try_from(inner.Anonymous.ulVal).ok(),
            VT_I4 => u16::try_from(inner.Anonymous.lVal).ok(),
            _ => None,
        }
    }
}

fn normalize_orientation(orientation: u16) -> u16 {
    if (1..=8).contains(&orientation) {
        orientation
    } else {
        1
    }
}

fn read_icc_profile(frame: &IWICBitmapFrameDecode) -> Option<Vec<u8>> {
    let mut contexts: Vec<_> = (0..8).map(|_| None).collect();
    let mut actual = 0;
    unsafe { frame.GetColorContexts(&mut contexts, &mut actual) }.ok()?;
    if actual as usize > contexts.len() {
        return None;
    }

    for context in contexts.into_iter().take(actual as usize).flatten() {
        let is_profile =
            unsafe { context.GetType() }.is_ok_and(|kind| kind == WICColorContextProfile);
        if !is_profile {
            continue;
        }
        let mut buffer = vec![0u8; INITIAL_ICC_BUFFER];
        let mut length = 0u32;
        let first = unsafe { context.GetProfileBytes(&mut buffer, &mut length) };
        if first.is_err() && length as usize > buffer.len() && length as usize <= MAX_ICC_BYTES {
            buffer.resize(length as usize, 0);
            let second = unsafe { context.GetProfileBytes(&mut buffer, &mut length) };
            if second.is_err() {
                return None;
            }
        } else if first.is_err() {
            return None;
        }
        let length = length as usize;
        if length == 0 || length > buffer.len() {
            return None;
        }
        buffer.truncate(length);
        return Some(buffer);
    }
    None
}

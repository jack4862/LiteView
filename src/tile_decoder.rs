use std::sync::Arc;

use crate::{
    loader, orientation,
    tile_service::{TileDecodeRequest, TileDecoder},
};

#[cfg(windows)]
pub struct JpegTileDecoder;

#[cfg(windows)]
impl TileDecoder for JpegTileDecoder {
    fn decode(&self, request: &TileDecodeRequest) -> Result<Vec<u8>, String> {
        use crate::wic;
        use image::{RgbaImage, imageops::FilterType};

        let [_, _, display_width, display_height] = request.source_rect;
        let [output_width, output_height] = request.output_size;
        if display_width == 0
            || display_height == 0
            || output_width == 0
            || output_height == 0
            || request.source_dimensions[0] == 0
            || request.source_dimensions[1] == 0
        {
            return Err("tile request contains zero dimensions".to_owned());
        }

        let source_rect = orientation::display_rect_to_source(
            request.source_rect,
            request.source_dimensions,
            request.exif_orientation,
        );
        let raw_target = if orientation::normalized(request.exif_orientation) >= 5 {
            [output_height, output_width]
        } else {
            [output_width, output_height]
        };
        let decoded = wic::decode_jpeg_region(
            &request.path,
            wic::SourceRect::new(
                source_rect[0],
                source_rect[1],
                source_rect[2],
                source_rect[3],
            ),
            raw_target,
        )
        .map_err(|error| error.to_string())?;
        let expected = (decoded.output_dimensions[0] as usize)
            .checked_mul(decoded.output_dimensions[1] as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "tile output dimensions overflowed".to_owned())?;
        if decoded.pixels.len() != expected {
            return Err(format!(
                "tile decoder returned {} bytes; expected {expected}",
                decoded.pixels.len()
            ));
        }

        let image = RgbaImage::from_raw(
            decoded.output_dimensions[0],
            decoded.output_dimensions[1],
            decoded.pixels,
        )
        .ok_or_else(|| "tile decoder returned an invalid pixel buffer".to_owned())?;
        let resized =
            image::imageops::resize(&image, raw_target[0], raw_target[1], FilterType::Triangle)
                .into_raw();
        let (mut rgba, dimensions) =
            orientation::apply_orientation(&resized, raw_target, request.exif_orientation)
                .map_err(|error| error.to_string())?;
        if dimensions != [output_width, output_height] {
            return Err(format!(
                "tile orientation produced {}x{}; expected {output_width}x{output_height}",
                dimensions[0], dimensions[1]
            ));
        }
        loader::convert_to_srgb(&mut rgba, request.icc_profile.as_deref());
        Ok(rgba)
    }
}

pub struct PreviewOnlyTileDecoder;

impl TileDecoder for PreviewOnlyTileDecoder {
    fn decode(&self, _request: &TileDecodeRequest) -> Result<Vec<u8>, String> {
        Err("detail tiles are unavailable on this platform".to_owned())
    }
}

pub fn platform_decoder() -> Arc<dyn TileDecoder> {
    #[cfg(windows)]
    {
        Arc::new(JpegTileDecoder)
    }
    #[cfg(not(windows))]
    {
        Arc::new(PreviewOnlyTileDecoder)
    }
}

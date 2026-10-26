use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OrientationError {
    #[error("invalid image dimensions {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("RGBA buffer has {actual} bytes; expected {expected}")]
    InvalidBuffer { expected: usize, actual: usize },
}

pub fn normalized(orientation: u8) -> u8 {
    if (1..=8).contains(&orientation) {
        orientation
    } else {
        1
    }
}

pub fn oriented_dimensions(dimensions: [u32; 2], orientation: u8) -> [u32; 2] {
    if normalized(orientation) >= 5 {
        [dimensions[1], dimensions[0]]
    } else {
        dimensions
    }
}

pub fn apply_orientation(
    rgba: &[u8],
    dimensions: [u32; 2],
    orientation: u8,
) -> Result<(Vec<u8>, [u32; 2]), OrientationError> {
    let [width, height] = dimensions;
    if width == 0 || height == 0 {
        return Err(OrientationError::InvalidDimensions { width, height });
    }
    let expected =
        pixel_bytes(width, height).ok_or(OrientationError::InvalidDimensions { width, height })?;
    if rgba.len() != expected {
        return Err(OrientationError::InvalidBuffer {
            expected,
            actual: rgba.len(),
        });
    }

    let orientation = normalized(orientation);
    if orientation == 1 {
        return Ok((rgba.to_vec(), dimensions));
    }
    let output_dimensions = oriented_dimensions(dimensions, orientation);
    let mut output = vec![0; expected];
    for display_y in 0..output_dimensions[1] {
        for display_x in 0..output_dimensions[0] {
            let (source_x, source_y) =
                source_pixel(display_x, display_y, width, height, orientation);
            let source = pixel_index(source_x, source_y, width);
            let destination = pixel_index(display_x, display_y, output_dimensions[0]);
            output[destination..destination + 4].copy_from_slice(&rgba[source..source + 4]);
        }
    }
    Ok((output, output_dimensions))
}

pub fn display_rect_to_source(
    rect: [u32; 4],
    source_dimensions: [u32; 2],
    orientation: u8,
) -> [u32; 4] {
    let [x, y, width, height] = rect;
    let [source_width, source_height] = source_dimensions;
    let orientation = normalized(orientation);
    let [display_width, display_height] = oriented_dimensions(source_dimensions, orientation);
    if source_width == 0
        || source_height == 0
        || width == 0
        || height == 0
        || x >= display_width
        || y >= display_height
    {
        return [0, 0, 0, 0];
    }

    // Map the first and last display pixel to source coordinates. Both axes
    // of every EXIF transform are monotonic, so these corners define the
    // complete source bounding box.
    let start_x = x;
    let start_y = y;
    let end_x = x.saturating_add(width).min(display_width) - 1;
    let end_y = y.saturating_add(height).min(display_height) - 1;
    let corners = [
        source_pixel(start_x, start_y, source_width, source_height, orientation),
        source_pixel(start_x, end_y, source_width, source_height, orientation),
        source_pixel(end_x, start_y, source_width, source_height, orientation),
        source_pixel(end_x, end_y, source_width, source_height, orientation),
    ];
    let min_x = corners.iter().map(|corner| corner.0).min().unwrap_or(0);
    let min_y = corners.iter().map(|corner| corner.1).min().unwrap_or(0);
    let max_x = corners.iter().map(|corner| corner.0).max().unwrap_or(0);
    let max_y = corners.iter().map(|corner| corner.1).max().unwrap_or(0);

    [min_x, min_y, max_x - min_x + 1, max_y - min_y + 1]
}

fn source_pixel(x: u32, y: u32, width: u32, height: u32, orientation: u8) -> (u32, u32) {
    match orientation {
        2 => (width - 1 - x, y),
        3 => (width - 1 - x, height - 1 - y),
        4 => (x, height - 1 - y),
        5 => (y, x),
        6 => (y, height - 1 - x),
        7 => (width - 1 - y, height - 1 - x),
        8 => (width - 1 - y, x),
        _ => (x, y),
    }
}

fn pixel_bytes(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

fn pixel_index(x: u32, y: u32, width: u32) -> usize {
    ((y as usize * width as usize) + x as usize) * 4
}

#[cfg(test)]
mod tests {
    use super::{apply_orientation, display_rect_to_source, oriented_dimensions};

    #[test]
    fn applies_every_exif_orientation() {
        let pixels = [1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255];
        let expected_first = [
            1, 2, 3, 4, 2, 1, 4, 3, 4, 3, 2, 1, 3, 4, 1, 2, 1, 3, 2, 4, 3, 1, 4, 2, 4, 2, 3, 1, 2,
            4, 1, 3,
        ];

        for orientation in 1..=8 {
            let (oriented, dimensions) =
                apply_orientation(&pixels, [2, 2], orientation).expect("orient pixels");
            let red: Vec<_> = oriented
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| pixel[0])
                .collect();
            assert_eq!(dimensions, oriented_dimensions([2, 2], orientation));
            assert_eq!(red, expected_first[(orientation as usize - 1) * 4..][..4]);
        }
    }

    #[test]
    fn maps_display_rectangles_back_to_each_source_orientation() {
        let rect = [1, 0, 1, 2];
        let expected = [
            [1, 0, 1, 2],
            [2, 0, 1, 2],
            [2, 0, 1, 2],
            [1, 0, 1, 2],
            [0, 1, 2, 1],
            [0, 0, 2, 1],
            [2, 0, 2, 1],
            [2, 1, 2, 1],
        ];

        for orientation in 1..=8 {
            assert_eq!(
                display_rect_to_source(rect, [4, 2], orientation),
                expected[orientation as usize - 1]
            );
        }
    }

    #[test]
    fn maps_out_of_bounds_rectangles_to_empty_source_rectangles() {
        assert_eq!(
            display_rect_to_source([4, 0, 1, 1], [4, 2], 1),
            [0, 0, 0, 0]
        );
        assert_eq!(
            display_rect_to_source([0, 0, 0, 1], [4, 2], 1),
            [0, 0, 0, 0]
        );
        assert_eq!(
            display_rect_to_source([0, 0, 1, 1], [0, 2], 1),
            [0, 0, 0, 0]
        );
    }
}

use std::{fs::File, io::BufReader, path::Path};

use exif::{Exif, In, Tag};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataSummary {
    pub camera: Option<String>,
    pub captured_at: Option<String>,
    pub exposure: Option<String>,
    pub aperture: Option<String>,
    pub iso: Option<String>,
    pub lens: Option<String>,
}

pub fn read_summary(path: &Path) -> MetadataSummary {
    let Ok(file) = File::open(path) else {
        return MetadataSummary::default();
    };
    let Ok(exif) = exif::Reader::new().read_from_container(&mut BufReader::new(file)) else {
        return MetadataSummary::default();
    };

    MetadataSummary {
        camera: camera_name(&exif),
        captured_at: field_text(&exif, Tag::DateTimeOriginal),
        exposure: field_text(&exif, Tag::ExposureTime),
        aperture: field_text(&exif, Tag::FNumber),
        iso: field_text(&exif, Tag::PhotographicSensitivity),
        lens: field_text(&exif, Tag::LensModel),
    }
}

fn camera_name(exif: &Exif) -> Option<String> {
    let make = field_text(exif, Tag::Make);
    let model = field_text(exif, Tag::Model);
    match (make, model) {
        (Some(make), Some(model))
            if !model
                .to_ascii_lowercase()
                .contains(&make.to_ascii_lowercase()) =>
        {
            Some(format!("{make} {model}"))
        }
        (_, Some(model)) => Some(model),
        (Some(make), None) => Some(make),
        (None, None) => None,
    }
}

fn field_text(exif: &Exif, tag: Tag) -> Option<String> {
    exif.get_field(tag, In::PRIMARY)
        .map(|field| field.display_value().with_unit(exif).to_string())
        .filter(|value| !value.trim().is_empty())
}

use eframe::egui;

pub const APP_ID: &str = "LiteView";
pub const MULTI_INSTANCE_POLICY: &str =
    "Each invocation opens an independent window; no remote commands or IPC are accepted.";

pub fn app_icon() -> egui::IconData {
    const EMBEDDED_ICON: &[u8] = include_bytes!("../assets/icon.png");
    const RUNTIME_ICON_SIZE: u32 = 256;

    let image = image::load_from_memory(EMBEDDED_ICON).expect("embedded icon is a valid image");
    let image = image.resize(
        RUNTIME_ICON_SIZE,
        RUNTIME_ICON_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    let rgba_image = image.to_rgba8();
    let (width, height) = rgba_image.dimensions();
    let rgba = rgba_image.into_raw();

    egui::IconData {
        rgba,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::app_icon;

    #[test]
    fn embedded_icon_has_matching_rgba_and_transparent_corner() {
        let icon = app_icon();

        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        assert_eq!(&icon.rgba[..4], &[0, 0, 0, 0]);
    }
}

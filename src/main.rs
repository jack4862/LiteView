#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    error::Error,
    io, panic,
    path::{Path, PathBuf},
};

use clap::Parser;
use eframe::egui;
use liteview::{
    app::LiteViewApp,
    benchmark,
    loader::IccStatus,
    platform::{APP_ID, app_icon},
};

#[derive(Debug, Parser)]
#[command(
    name = "liteview",
    version,
    about = "Fast, local-first JPEG, PNG, and WebP image viewer"
)]
struct Cli {
    /// Image to open.
    #[arg(value_name = "IMAGE", conflicts_with = "benchmark")]
    image: Option<PathBuf>,

    /// Decode an image once and print timing and resource information.
    #[arg(long, value_name = "IMAGE")]
    benchmark: Option<PathBuf>,

    /// Number of benchmark samples to run.
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 1,
        requires = "benchmark"
    )]
    iterations: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    attach_parent_console();

    let cli = Cli::parse();
    if let Some(path) = cli.benchmark {
        return run_benchmark(&path, cli.iterations);
    }

    install_panic_dialog();

    let mut initial_path = cli.image;
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_icon(app_icon())
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([720.0, 480.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "LiteView",
        native_options,
        Box::new(move |creation| {
            let app = LiteViewApp::new(creation, initial_path.take())?;
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )?;
    Ok(())
}

#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut core::ffi::c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn GetStdHandle(standard_handle: u32) -> *mut core::ffi::c_void;
        fn SetStdHandle(standard_handle: u32, handle: *mut core::ffi::c_void) -> i32;
    }

    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;

    // Release builds use the GUI subsystem so Explorer launches no console window.
    // Attach one when a parent terminal exists, but preserve redirected CLI handles.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }

        let console_name = "CONOUT$\0".encode_utf16().collect::<Vec<_>>();
        let handle = CreateFileW(
            console_name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            core::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            core::ptr::null_mut(),
        );
        if handle as isize == -1 {
            return;
        }

        let output_handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if standard_handle_is_missing(output_handle) {
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
        }

        let error_handle = GetStdHandle(STD_ERROR_HANDLE);
        if standard_handle_is_missing(error_handle) {
            SetStdHandle(STD_ERROR_HANDLE, handle);
        }
    }
}

#[cfg(any(test, all(windows, not(debug_assertions))))]
fn standard_handle_is_missing(handle: *mut core::ffi::c_void) -> bool {
    let value = handle as isize;
    value == 0 || value == -1
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_parent_console() {}

fn run_benchmark(path: &Path, iterations: usize) -> Result<(), Box<dyn Error>> {
    let report =
        benchmark::run(path, iterations).map_err(|error| io::Error::other(error.to_string()))?;
    let icc = match report.icc_status {
        IccStatus::Absent => "absent",
        IccStatus::Converted => "converted",
        IccStatus::Invalid => "invalid",
    };
    println!("format={}", report.format.label());
    println!(
        "source_dimensions={}x{}",
        report.source_dimensions[0], report.source_dimensions[1]
    );
    println!(
        "display_dimensions={}x{}",
        report.display_dimensions[0], report.display_dimensions[1]
    );
    println!(
        "preview_dimensions={}x{}",
        report.preview_dimensions[0], report.preview_dimensions[1]
    );
    println!("file_bytes={}", report.file_size);
    println!("icc={icc}");
    println!("downsampled={}", report.downsampled);
    println!("iterations={}", report.iterations);
    println!("elapsed_ms_min={}", report.min.as_millis());
    println!("elapsed_ms_p50={}", report.p50.as_millis());
    println!("elapsed_ms_p95={}", report.p95.as_millis());
    println!("elapsed_ms_max={}", report.max.as_millis());
    Ok(())
}

fn install_panic_dialog() {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        previous_hook(info);
        show_panic_dialog(&panic_dialog_message(info.location()));
    }));
}

fn panic_dialog_message(location: Option<&panic::Location<'_>>) -> String {
    match location {
        Some(location) => format!(
            "LiteView hit an unexpected problem at {}:{}:{}.",
            location.file(),
            location.line(),
            location.column()
        ),
        None => "LiteView hit an unexpected problem at an unknown location.".to_owned(),
    }
}

#[cfg(windows)]
fn show_panic_dialog(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    const MB_ICONERROR: u32 = 0x0000_0010;
    let text = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let caption = "LiteView\0".encode_utf16().collect::<Vec<_>>();
    unsafe {
        MessageBoxW(
            core::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_ICONERROR,
        )
    };
}

#[cfg(not(windows))]
fn show_panic_dialog(_message: &str) {}

#[cfg(test)]
mod tests {
    use super::{panic_dialog_message, standard_handle_is_missing};
    use std::panic::Location;

    #[test]
    fn preserves_existing_standard_handles_and_replaces_missing_handles() {
        let mut valid_handle = 7_u8;
        let valid_pointer = &mut valid_handle as *mut u8 as *mut core::ffi::c_void;

        assert!(standard_handle_is_missing(core::ptr::null_mut()));
        assert!(standard_handle_is_missing(
            -1_isize as *mut core::ffi::c_void
        ));
        assert!(!standard_handle_is_missing(valid_pointer));
    }

    #[test]
    fn panic_dialog_reports_location_without_payload_or_paths() {
        let location = Location::caller();
        let message = panic_dialog_message(Some(location));

        assert!(message.starts_with("LiteView hit an unexpected problem at "));
        assert!(message.contains(location.file()));
        assert!(!message.contains("user image"));
    }
}

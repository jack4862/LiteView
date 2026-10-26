#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const MAGIC: &[u8; 8] = b"LVPKG001";
const FOOTER_LENGTH: usize = 20;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, PartialEq, Eq)]
struct PackageFile {
    name: String,
    contents: Vec<u8>,
}

struct PayloadReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> PayloadReader<'a> {
    fn new(bytes: &'a [u8], cursor: usize) -> Self {
        Self { bytes, cursor }
    }

    fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .cursor
            .checked_add(length)
            .ok_or_else(|| "package entry length overflowed".to_owned())?;
        if end > self.bytes.len() {
            return Err(format!(
                "package entry extends beyond the payload (cursor={}; requested={length}; payload={})",
                self.cursor,
                self.bytes.len()
            ));
        }

        let value = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(value)
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
    }
}

fn parse_payload(bytes: &[u8]) -> Result<Vec<PackageFile>, String> {
    if bytes.len() < FOOTER_LENGTH {
        return Err("invalid LiteView package footer".to_owned());
    }

    let footer = &bytes[bytes.len() - FOOTER_LENGTH..];
    if &footer[FOOTER_LENGTH - MAGIC.len()..] != MAGIC {
        return Err("invalid LiteView package footer".to_owned());
    }

    let file_count = u32::from_le_bytes(footer[0..4].try_into().expect("four bytes"));
    let payload_start = usize::try_from(u64::from_le_bytes(
        footer[4..12].try_into().expect("eight bytes"),
    ))
    .map_err(|_| "package payload start exceeds the address space".to_owned())?;
    if payload_start > bytes.len() - FOOTER_LENGTH {
        return Err("package payload start is outside the file".to_owned());
    }

    let payload_end = bytes.len() - FOOTER_LENGTH;
    let mut reader = PayloadReader::new(bytes, payload_start);
    let mut files = Vec::with_capacity(usize::try_from(file_count).unwrap_or(1024).min(1024));
    for _ in 0..file_count {
        let name_length = reader.read_u32()? as usize;
        let contents_length = usize::try_from(reader.read_u64()?)
            .map_err(|_| "package file length exceeds the address space".to_owned())?;
        let name_bytes = reader.read_bytes(name_length)?.to_vec();
        let name = String::from_utf8(name_bytes)
            .map_err(|_| "package file name is not UTF-8".to_owned())?;
        if !is_safe_package_name(&name) {
            return Err(format!("package contains an unsafe file name: {name}"));
        }
        let contents = reader.read_bytes(contents_length)?.to_vec();
        files.push(PackageFile { name, contents });
    }

    if reader.cursor != payload_end {
        return Err("package payload has unexpected trailing bytes".to_owned());
    }
    Ok(files)
}

fn is_safe_package_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            == Some(name)
}

fn read_package() -> Result<Vec<PackageFile>, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let bytes = fs::read(&executable).map_err(|error| error.to_string())?;
    parse_payload(&bytes)
}

fn extract_package(files: &[PackageFile]) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let staging = std::env::temp_dir().join(format!(
        "LiteView-install-{}-{timestamp}",
        std::process::id()
    ));
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    for file in files {
        fs::write(staging.join(&file.name), &file.contents)
            .map_err(|error| format!("could not write {}: {error}", file.name))?;
    }
    Ok(staging)
}

fn run_installer_script(staging: &Path) -> Result<(), String> {
    let script = staging.join("install.ps1");
    let output = Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail = stderr
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "install.ps1 exited with {}. {tail}",
        output.status.code().unwrap_or(-1)
    ))
}

fn installed_executable() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Programs")
        .join("LiteView")
        .join("liteview.exe")
}

fn run() -> Result<(), String> {
    let files = read_package()?;
    if files.is_empty() {
        return Err("The installer package is empty.".to_owned());
    }

    let staging = extract_package(&files)?;
    let result = run_installer_script(&staging);
    let _ = fs::remove_dir_all(staging);
    result?;
    show_message(
        &format!(
            "Installation complete.\nLiteView was installed at:\n{}",
            installed_executable().display()
        ),
        true,
    );
    Ok(())
}

#[cfg(windows)]
fn show_message(message: &str, success: bool) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    let icon = if success { 0x0000_0040 } else { 0x0000_0010 };
    let text = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let caption = "LiteView Setup"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe { MessageBoxW(core::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), icon) };
}

#[cfg(not(windows))]
fn show_message(_message: &str, _success: bool) {}

fn main() {
    if let Err(error) = run() {
        show_message(&format!("Installation failed.\n\n{error}"), false);
    }
}

#[cfg(test)]
mod tests {
    use super::{PackageFile, parse_payload};

    #[test]
    fn parses_appended_package_files_and_rejects_unknown_footers() {
        let file = PackageFile {
            name: "README.md".to_owned(),
            contents: b"LiteView".to_vec(),
        };
        let mut bytes = vec![0_u8; 7];
        bytes.extend_from_slice(&(file.name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(file.contents.len() as u64).to_le_bytes());
        bytes.extend_from_slice(file.name.as_bytes());
        bytes.extend_from_slice(&file.contents);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&7_u64.to_le_bytes());
        bytes.extend_from_slice(b"LVPKG001");

        assert_eq!(parse_payload(&bytes), Ok(vec![file]));
        assert_eq!(
            parse_payload(&bytes[..bytes.len() - 1]),
            Err("invalid LiteView package footer".to_owned())
        );
    }
}

use std::fs::OpenOptions;
use std::io::Write;

use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::paths;

/// Local wall clock time as `YYYY-MM-DD HH:MM:SS`.
pub fn local_time() -> String {
    let time = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
    )
}

/// Appends one timestamped line to `%APPDATA%\GModeSwitcher\gmode-switcher.log`.
/// A windowless tray app fails silently, so this is the only diagnostic channel;
/// any I/O error is swallowed on purpose.
pub fn line(message: &str) {
    let Some(dir) = paths::data_dir() else {
        return;
    };
    let entry = format!("{} {}\r\n", local_time(), message);
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("gmode-switcher.log"))
        .and_then(|mut file| file.write_all(entry.as_bytes()));
}

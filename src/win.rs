use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE, SetLastError,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW, SW_SHOWNORMAL,
};
use windows::core::PCWSTR;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// Modal dialog, used for the few unrecoverable errors of a windowless program.
pub fn error_box(title: &str, text: &str) {
    let text = wide(text);
    let title = wide(title);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

/// Handle holding the single-instance mutex; released when dropped.
pub struct SingleInstanceGuard(Option<HANDLE>);

impl SingleInstanceGuard {
    /// Returns `None` when another instance already holds the mutex.
    pub fn acquire() -> Option<Self> {
        let name = wide("GModeSwitcher.SingleInstance.v1");
        unsafe {
            SetLastError(ERROR_SUCCESS);
            match CreateMutexW(None, false, PCWSTR(name.as_ptr())) {
                Ok(handle) => {
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        let _ = CloseHandle(handle);
                        None
                    } else {
                        Some(Self(Some(handle)))
                    }
                }
                // Can't tell whether one is running: better to start than to refuse to.
                Err(_) => Some(Self(None)),
            }
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE(std::ptr::null_mut());
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Relaunches the current executable with the `runas` verb, which triggers UAC.
pub fn relaunch_elevated() -> bool {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return false,
    };
    let verb = wide("runas");
    let file = wide(exe);
    unsafe {
        let result = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        result.0 as isize > 32
    }
}

/// Runs a console program without flashing a console window; returns its exit code.
pub fn run_hidden(program: &str, args: &[&str]) -> Option<i32> {
    let output = std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    Some(output.status.code().unwrap_or(1))
}

/// Same as [`run_hidden`], but also returns the combined output decoded lossily for logging.
pub fn run_hidden_captured(program: &str, args: &[&str]) -> Option<(i32, String)> {
    let output = std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Some((output.status.code().unwrap_or(1), text.trim().to_string()))
}

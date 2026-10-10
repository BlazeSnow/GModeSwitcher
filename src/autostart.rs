use std::path::Path;

use crate::log;
use crate::settings;
use crate::win;

pub const TASK_NAME: &str = "GModeSwitcher";

/// Same settings as the original tcc-g15 task: run at logon with the highest
/// available rights, never killed for running too long, never blocked by battery.
const TASK_XML: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Date>{DATE}</Date>
    <Description>GModeSwitcher - Dell G15 thermal mode switcher tray app</Description>
    <URI>\GModeSwitcher</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <StartBoundary>{DATE}</StartBoundary>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>true</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{EXE}"</Command>
    </Exec>
  </Actions>
</Task>
"#;

/// Whether the logon task exists, decided by `schtasks`' exit code alone.
///
/// Its output must never be parsed. Measured on this Chinese Windows: `schtasks`
/// prints in the system ANSI code page, not in UTF-8 and not even in the UTF-16 its
/// own task XML declares. A full `/query /xml` dump was 325869 bytes with 396
/// non-ASCII bytes, it fails to decode as UTF-8 (`invalid start byte 0xb5`) and
/// decodes fine as GBK, e.g. `<Author>Mozilla</Author>`. A lossy UTF-8 decode would
/// turn such text into replacement characters, so comparing it against our own path
/// would never match and the task would be rewritten on every single start. The path
/// we register is therefore remembered in `settings.json` instead.
pub fn is_enabled() -> bool {
    win::run_hidden("schtasks", &["/query", "/tn", TASK_NAME]) == Some(0)
}

/// Startup self-healing: if the task exists but is registered with a different
/// executable path than the one running now, recreate it.
///
/// Returns whether autostart ends up enabled, which is what the menu check mark
/// shows — the query happens exactly once, so starting up does not spawn `schtasks`
/// twice. Needs administrator rights (recreating the task does), so it must be called
/// after the elevation check. Failures are logged only: this is a repair of a working
/// setup, and the task itself still exists even if the rewrite does not succeed.
pub fn reconcile_at_startup() -> bool {
    if !is_enabled() {
        log::line("autostart: no logon task found");
        return false;
    }

    let current = match current_exe_path() {
        Ok(current) => current,
        Err(error) => {
            log::line(&format!("autostart: {error}"));
            return true;
        }
    };

    let recorded = settings::recorded_autostart_path();
    if !needs_rewrite(recorded.as_deref(), &current) {
        log::line("autostart: the registered path is still correct");
        return true;
    }

    log::line(&format!(
        "autostart: registered path {recorded:?} does not match {current:?}, recreating the task"
    ));
    match enable() {
        Ok(()) => {
            log::line("autostart: the task was updated to the current path");
            true
        }
        Err(error) => {
            log::line(&format!("autostart: could not update the task: {error}"));
            true
        }
    }
}

pub fn enable() -> Result<(), String> {
    let exe = current_exe_path()?;
    let xml_path = std::env::temp_dir().join("gmode-switcher-task.xml");
    write_task_xml(&xml_path, &exe).map_err(|error| format!("无法写入计划任务 XML：{error}"))?;

    let xml_path = xml_path.to_string_lossy().into_owned();
    run(&["/create", "/xml", xml_path.as_str(), "/tn", TASK_NAME, "/f"])?;
    // Remember what was registered; this record is the only reliable source for the
    // path, because `schtasks` output cannot be decoded (see `is_enabled`).
    settings::record_autostart_path(Some(&exe));
    log::line("scheduled task created");
    Ok(())
}

pub fn disable() -> Result<(), String> {
    run(&["/delete", "/tn", TASK_NAME, "/f"])?;
    settings::record_autostart_path(None);
    log::line("scheduled task deleted");
    Ok(())
}

fn current_exe_path() -> Result<String, String> {
    std::env::current_exe()
        .map(|exe| exe.to_string_lossy().into_owned())
        .map_err(|error| format!("无法获取程序自身路径：{error}"))
}

/// Whether the task has to be recreated for `current`.
///
/// An unknown recorded path (older `settings.json`, or the file was deleted) counts
/// as "rewrite once" so the task ends up pointing at this executable. Comparison is
/// case-insensitive because Windows paths are: the drive letter and directory casing
/// reported by `current_exe()` can differ from what was recorded, and a strict
/// comparison would rewrite the task needlessly.
fn needs_rewrite(recorded: Option<&str>, current: &str) -> bool {
    match recorded {
        Some(recorded) => !recorded.eq_ignore_ascii_case(current),
        None => true,
    }
}

fn run(args: &[&str]) -> Result<(), String> {
    match win::run_hidden_captured("schtasks", args) {
        Some((0, _)) => Ok(()),
        Some((code, output)) => {
            // The captured text is in the ANSI code page, so it can look garbled in
            // the log; the exit code is the part that decides anything.
            log::line(&format!("schtasks {args:?} exited with {code}: {output}"));
            Err(format!("schtasks 返回错误码 {code}"))
        }
        None => Err("无法启动 schtasks".to_string()),
    }
}

/// `schtasks` validates the XML against the encoding declared in it, so the file
/// is written as UTF-16LE with a BOM by hand.
fn write_task_xml(path: &Path, exe: &str) -> std::io::Result<()> {
    let stamp = log::local_time().replace(' ', "T");
    let xml = TASK_XML.replace("{DATE}", &stamp).replace("{EXE}", exe);

    let mut bytes = Vec::with_capacity(xml.len() * 2 + 2);
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const EXE: &str = r"C:\Program Files\GModeSwitcher\GModeSwitcher.exe";

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("gmode-switcher-{}-{name}.xml", std::process::id()))
    }

    /// Writes the task XML for `EXE` and returns the raw bytes plus the decoded text.
    /// The temporary file is removed again, so nothing is left behind.
    fn write_and_read_back(name: &str) -> (Vec<u8>, String) {
        let path = temp_path(name);
        write_task_xml(&path, EXE).expect("writing the task XML has to succeed");
        let bytes = std::fs::read(&path).expect("the task XML has to be readable");
        let _ = std::fs::remove_file(&path);

        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let text = String::from_utf16(&units).expect("the task XML has to be valid UTF-16");
        (bytes, text)
    }

    #[test]
    fn task_file_is_utf16le_with_bom() {
        let (bytes, _) = write_and_read_back("bom");
        assert!(bytes.len() > 2, "the file must not be empty");
        assert_eq!(
            bytes[..2],
            [0xFF, 0xFE],
            "the file has to start with the UTF-16LE BOM"
        );
        assert_eq!(
            bytes.len() % 2,
            0,
            "UTF-16LE text has an even number of bytes"
        );
    }

    #[test]
    fn task_xml_contains_the_required_settings() {
        let (_, xml) = write_and_read_back("settings");
        for expected in [
            "<RunLevel>HighestAvailable</RunLevel>",
            "<LogonTrigger>",
            "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
        ] {
            assert!(xml.contains(expected), "missing {expected} in:\n{xml}");
        }
    }

    #[test]
    fn task_xml_runs_the_given_executable_without_arguments() {
        let (_, xml) = write_and_read_back("command");
        assert!(
            xml.contains(&format!("<Command>\"{EXE}\"</Command>")),
            "the command has to be the quoted executable path:\n{xml}"
        );
        assert!(
            !xml.contains("<Arguments>"),
            "the task must not pass any command line arguments:\n{xml}"
        );
    }

    #[test]
    fn task_xml_has_no_leftover_placeholders() {
        let (_, xml) = write_and_read_back("placeholders");
        for placeholder in ["{DATE}", "{EXE}"] {
            assert!(
                !xml.contains(placeholder),
                "unreplaced {placeholder} in:\n{xml}"
            );
        }
    }

    #[test]
    fn the_task_is_left_alone_when_the_recorded_path_matches() {
        assert!(!needs_rewrite(Some(EXE), EXE));
    }

    /// Windows paths are case-insensitive, and `current_exe()` casing depends on how
    /// the process was launched; a differing case must not trigger a rewrite.
    #[test]
    fn the_task_is_left_alone_when_only_the_casing_differs() {
        assert!(!needs_rewrite(
            Some(r"C:\Program Files\GModeSwitcher\GModeSwitcher.exe"),
            r"c:\program files\gmodeswitcher\GMODESWITCHER.EXE"
        ));
    }

    #[test]
    fn the_task_is_rewritten_when_the_executable_moved() {
        assert!(needs_rewrite(Some(r"C:\Old\GModeSwitcher.exe"), EXE));
    }

    /// Nothing recorded: an older settings file or a deleted one. Rewrite once so the
    /// task ends up pointing at the executable that is running now.
    #[test]
    fn the_task_is_rewritten_when_no_path_was_recorded() {
        assert!(needs_rewrite(None, EXE));
    }
}

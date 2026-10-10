use std::path::Path;

use crate::log;
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

/// Queried by exit code only, so no localized output has to be parsed.
pub fn is_enabled() -> bool {
    win::run_hidden("schtasks", &["/query", "/tn", TASK_NAME]) == Some(0)
}

pub fn enable() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| format!("无法获取程序自身路径：{error}"))?;
    let xml_path = std::env::temp_dir().join("gmode-switcher-task.xml");
    write_task_xml(&xml_path, &exe.to_string_lossy())
        .map_err(|error| format!("无法写入计划任务 XML：{error}"))?;

    let xml_path = xml_path.to_string_lossy().into_owned();
    run(&["/create", "/xml", xml_path.as_str(), "/tn", TASK_NAME, "/f"])?;
    log::line("scheduled task created");
    Ok(())
}

pub fn disable() -> Result<(), String> {
    run(&["/delete", "/tn", TASK_NAME, "/f"])?;
    log::line("scheduled task deleted");
    Ok(())
}

fn run(args: &[&str]) -> Result<(), String> {
    match win::run_hidden_captured("schtasks", args) {
        Some((0, _)) => Ok(()),
        Some((code, output)) => {
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
}

use std::path::PathBuf;
use std::sync::OnceLock;

/// `%APPDATA%\GModeSwitcher`, created on first use.
pub fn data_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("GModeSwitcher");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    })
    .as_ref()
}

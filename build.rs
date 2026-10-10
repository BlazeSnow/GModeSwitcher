// Build script: embeds `icons/app.ico`, the version resource and `app.manifest` into the
// executable.
//
// The version conversion is shared with `src/build_meta.rs` through `include!` so that
// it can be unit-tested. Nothing else in this script is covered by `cargo test`, so keep
// the logic it contains trivial.

include!("src/build_meta.rs");

use std::env;
use std::path::Path;

use winresource::{VersionInfo, WindowsResource};

fn main() {
    println!("cargo:rerun-if-changed=icons/app.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=src/build_meta.rs");
    // The version comes from Cargo.toml, which `version.ps1` syncs from VERSION.
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

    let version = env::var("CARGO_PKG_VERSION").expect("cargo should provide CARGO_PKG_VERSION");
    let quad = file_version_quad(&version);
    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").expect("cargo should provide CARGO_MANIFEST_DIR");
    let icon = Path::new(&manifest_dir).join("icons").join("app.ico");
    let icon = icon.to_str().expect("the icon path should be valid UTF-8");

    // The manifest is plain text with `{{VERSION}}` and `{{ARCHITECTURE}}` placeholders;
    // the resource compiler gets the text with both filled in.
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let architecture = manifest_architecture(&target_arch).unwrap_or_else(|| {
        panic!(
            "未知的目标架构 {target_arch}：请在 src/build_meta.rs 的 manifest_architecture 里补上它"
        )
    });
    let manifest_path = Path::new(&manifest_dir).join("app.manifest");
    let manifest = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("无法读取 {}：{error}", manifest_path.display()))
        .replace("{{VERSION}}", &quad)
        .replace("{{ARCHITECTURE}}", architecture);
    if manifest.contains("{{") {
        panic!("{} 里还有未替换的占位符", manifest_path.display());
    }

    let mut resource = WindowsResource::new();
    resource
        .set_icon(icon)
        .set_manifest(&manifest)
        // zh-CN (`0x0804`) so the properties page reports 中文(简体) rather than
        // 英语(美国). The crate writes `#pragma code_page(65001)` and a code page of
        // 1200, so the Chinese description is stored as UTF-16 and stays readable.
        .set_language(0x0804)
        .set("FileDescription", "戴尔 G15 性能模式切换工具")
        .set("ProductName", "GModeSwitcher")
        .set("CompanyName", "BlazeSnow")
        .set("LegalCopyright", "Copyright (C) 2026 BlazeSnow")
        .set("OriginalFilename", "GModeSwitcher.exe")
        .set("InternalName", "GModeSwitcher")
        // String fields keep the full version (`1.0.0-beta.1`), the numeric ones cannot
        // carry a pre-release suffix and therefore get `1.0.0.0`.
        .set("FileVersion", &version)
        .set("ProductVersion", &version)
        // The manifest is only written out together with FILETYPE (it decides the
        // `1 24` resource line), so set it instead of relying on the default.
        .set_version_info(VersionInfo::FILETYPE, 1)
        .set_version_info(VersionInfo::FILEVERSION, pack_version(&quad))
        .set_version_info(VersionInfo::PRODUCTVERSION, pack_version(&quad));

    resource.compile().unwrap_or_else(|error| {
        panic!(
            "编译 Windows 资源失败：{error}\n\
             嵌入图标、清单与版本信息需要 Windows SDK 里的 rc.exe；安装 Visual Studio 或 \
             Windows SDK 即可（CI 的 windows-latest 自带）。\
             如果 rc.exe 装在非标准位置，可以用环境变量 RC_PATH 指定它。"
        );
    });
}

/// Packs `a.b.c.d` into the four 16-bit words a `VERSIONINFO` resource expects.
fn pack_version(quad: &str) -> u64 {
    let mut parts = quad.split('.').map(|part| part.parse::<u64>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0);

    let (major, minor, patch, release) = (next(), next(), next(), next());
    (major << 48) | (minor << 32) | (patch << 16) | release
}

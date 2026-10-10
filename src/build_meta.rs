// Build metadata that ends up in the executable's resources.
//
// `build.rs` embeds the icon and the version info, and shares the version conversion
// with this module through `include!` so that it can be unit-tested: build scripts are
// not covered by `cargo test`. Keep this file free of inner doc comments and of items
// the build script would not be able to compile.

/// Four-part numeric version for the `VERSIONINFO` resource.
///
/// The resource cannot express a pre-release suffix, so it is dropped and the missing
/// parts are filled with zeros: `1.0.0-beta.1` becomes `1.0.0.0`. The full version
/// string is kept in the string fields, which is what the file properties page shows.
// Used by `build.rs` through `include!` and by the tests below; the application itself
// has no use for it.
#[allow(dead_code)]
pub fn file_version_quad(version: &str) -> String {
    let numeric = version.split(['-', '+']).next().unwrap_or(version);

    let mut parts = [0u32; 4];
    for (index, part) in numeric.split('.').take(parts.len()).enumerate() {
        parts[index] = part.trim().parse().unwrap_or(0);
    }

    format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3])
}

#[cfg(test)]
mod tests {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::Shell::ExtractIconExW;
    use windows::core::PCWSTR;

    use super::*;

    #[test]
    fn a_pre_release_suffix_is_dropped_from_the_numeric_version() {
        assert_eq!(file_version_quad("1.0.0-beta.1"), "1.0.0.0");
        assert_eq!(file_version_quad("1.0.0-beta.1+build.7"), "1.0.0.0");
    }

    #[test]
    fn missing_parts_are_filled_with_zeros() {
        assert_eq!(file_version_quad("1.0.0"), "1.0.0.0");
        assert_eq!(file_version_quad("1.2"), "1.2.0.0");
        assert_eq!(file_version_quad("1"), "1.0.0.0");
    }

    #[test]
    fn four_part_versions_are_kept_as_they_are() {
        assert_eq!(file_version_quad("2.3.4"), "2.3.4.0");
        assert_eq!(file_version_quad("2.3.4.5"), "2.3.4.5");
    }

    #[test]
    fn unparsable_parts_become_zero_instead_of_panicking() {
        assert_eq!(file_version_quad(""), "0.0.0.0");
        assert_eq!(file_version_quad("1.x.3"), "1.0.3.0");
    }

    /// The resources are easy to lose silently (a missing `rc.exe`, a build script that
    /// stopped running), so check that the binary running these tests really carries an
    /// icon group. `-1` with no output buffers makes `ExtractIconExW` return the count.
    #[test]
    fn the_binary_has_an_embedded_icon_group() {
        let exe = std::env::current_exe().expect("the test binary path has to be known");
        let path: Vec<u16> = exe
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let groups = unsafe { ExtractIconExW(PCWSTR(path.as_ptr()), -1, None, None, 0) };
        assert!(
            groups >= 1,
            "the binary should carry at least one icon group, got {groups}"
        );
    }
}

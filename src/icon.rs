use std::io::Cursor;

use tray_icon::Icon;

use crate::thermal::ThermalMode;

/// Rendered from `icons/balanced.svg`: a white "G" on a blue rounded square.
const BALANCED_PNG: &[u8] = include_bytes!("../icons/balanced-32.png");
/// Rendered from `icons/gmode.svg`: a white "G" on a red rounded square.
const GMODE_PNG: &[u8] = include_bytes!("../icons/gmode-32.png");

/// Decodes a PNG into the RGBA bitmap the tray needs.
fn decode_rgba(png: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut reader = png::Decoder::new(Cursor::new(png))
        .read_info()
        .map_err(|error| format!("cannot read the embedded PNG: {error}"))?;

    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| "the embedded PNG is too large to decode".to_string())?;
    let mut buffer = vec![0; buffer_size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("cannot decode the embedded PNG: {error}"))?;

    if info.bit_depth != png::BitDepth::Eight {
        return Err("only 8-bit PNG assets are supported".to_string());
    }

    let pixels = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], u8::MAX])
            .collect(),
        color_type => return Err(format!("unsupported color type {color_type:?}")),
    };

    Ok((rgba, info.width, info.height))
}

/// Decodes the embedded icon belonging to `mode`: blue for Balanced, red for G-Mode.
///
/// A failure here only costs the icon (the tray still gets created), so it is
/// reported through the log instead of aborting the application.
pub fn tray_icon(mode: ThermalMode) -> Option<Icon> {
    let (png, name) = match mode {
        ThermalMode::Balanced => (BALANCED_PNG, "balanced"),
        ThermalMode::GMode => (GMODE_PNG, "G-mode"),
    };

    let (rgba, width, height) = match decode_rgba(png) {
        Ok(decoded) => decoded,
        Err(error) => {
            crate::log::line(&format!("icon: cannot decode the {name} icon: {error}"));
            return None;
        }
    };

    match Icon::from_rgba(rgba, width, height) {
        Ok(icon) => Some(icon),
        Err(error) => {
            crate::log::line(&format!(
                "icon: cannot create the {name} tray icon: {error}"
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every embedded icon, so a broken or swapped asset fails for both modes.
    const ICONS: [&[u8]; 2] = [BALANCED_PNG, GMODE_PNG];

    fn decoded(png: &[u8]) -> (Vec<u8>, u32, u32) {
        decode_rgba(png).expect("the embedded icon has to decode")
    }

    fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let index = (y * 32 + x) * 4;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    }

    #[test]
    fn both_embedded_pngs_decode_to_a_32x32_rgba_bitmap() {
        for png in ICONS {
            let (rgba, width, height) = decoded(png);
            assert_eq!(width, 32);
            assert_eq!(height, 32);
            assert_eq!(rgba.len(), 32 * 32 * 4);
        }
    }

    /// The icons are rounded squares, so their corners must be transparent; this also
    /// catches a replacement asset that is not an RGBA bitmap.
    #[test]
    fn both_embedded_pngs_have_a_transparent_corner() {
        for png in ICONS {
            let (rgba, _, _) = decoded(png);
            assert_eq!(
                pixel(&rgba, 0, 0)[3],
                0,
                "pixel (0, 0) should be fully transparent"
            );
        }
    }

    /// Balanced is blue and G-Mode is red. Sampling an opaque background pixel well
    /// away from the "G" catches the two assets being swapped or overwritten with the
    /// same image.
    #[test]
    fn the_two_icons_have_different_background_colours() {
        let (balanced, _, _) = decoded(BALANCED_PNG);
        let (g_mode, _, _) = decoded(GMODE_PNG);
        let balanced_background = pixel(&balanced, 4, 16);
        let g_mode_background = pixel(&g_mode, 4, 16);

        assert_eq!(
            balanced_background[3], 255,
            "balanced background has to be opaque"
        );
        assert_eq!(
            g_mode_background[3], 255,
            "G-mode background has to be opaque"
        );
        assert!(
            balanced_background[2] > balanced_background[0],
            "balanced (4, 16) should be blue-ish: {balanced_background:?}"
        );
        assert!(
            g_mode_background[0] > g_mode_background[2],
            "G-mode (4, 16) should be red-ish: {g_mode_background:?}"
        );
    }

    #[test]
    fn broken_input_is_reported_instead_of_panicking() {
        assert!(decode_rgba(&[]).is_err());
        assert!(decode_rgba(b"not a png at all").is_err());
    }
}

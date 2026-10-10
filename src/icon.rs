use std::io::Cursor;

use tray_icon::Icon;

/// Rendered from `icons/gmode.svg`: a white "G" on a rounded square.
const ICON_PNG: &[u8] = include_bytes!("../icons/gmode-32.png");

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

/// Decodes the embedded PNG into the RGBA bitmap the tray needs.
///
/// A failure here only costs the icon (the tray still gets created), so it is
/// reported through the log instead of aborting the application.
pub fn tray_icon() -> Option<Icon> {
    let (rgba, width, height) = match decode_rgba(ICON_PNG) {
        Ok(decoded) => decoded,
        Err(error) => {
            crate::log::line(&format!("icon: {error}"));
            return None;
        }
    };

    match Icon::from_rgba(rgba, width, height) {
        Ok(icon) => Some(icon),
        Err(error) => {
            crate::log::line(&format!("icon: cannot create the tray icon: {error}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_png_decodes_to_a_32x32_rgba_bitmap() {
        let (rgba, width, height) = decode_rgba(ICON_PNG).expect("the embedded icon has to decode");
        assert_eq!(width, 32);
        assert_eq!(height, 32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
    }

    /// The icon is a rounded square, so its corners must be transparent; this also
    /// catches a replacement asset that is not an RGBA bitmap.
    #[test]
    fn embedded_png_has_a_transparent_corner() {
        let (rgba, _, _) = decode_rgba(ICON_PNG).expect("the embedded icon has to decode");
        assert_eq!(rgba[3], 0, "pixel (0, 0) should be fully transparent");
    }

    #[test]
    fn broken_input_is_reported_instead_of_panicking() {
        assert!(decode_rgba(&[]).is_err());
        assert!(decode_rgba(b"not a png at all").is_err());
    }
}

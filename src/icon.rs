use std::io::Cursor;

use tray_icon::Icon;

/// Rendered from `icons/gmode.svg`: a white "G" on a rounded square.
const ICON_PNG: &[u8] = include_bytes!("../icons/gmode-32.png");

/// Decodes the embedded PNG into the RGBA bitmap the tray needs.
///
/// A failure here only costs the icon (the tray still gets created), so it is
/// reported through the log instead of aborting the application.
pub fn tray_icon() -> Option<Icon> {
    let mut reader = match png::Decoder::new(Cursor::new(ICON_PNG)).read_info() {
        Ok(reader) => reader,
        Err(error) => {
            crate::log::line(&format!("icon: cannot read the embedded PNG: {error}"));
            return None;
        }
    };

    let Some(buffer_size) = reader.output_buffer_size() else {
        crate::log::line("icon: the embedded PNG is too large to decode");
        return None;
    };

    let mut buffer = vec![0; buffer_size];
    let info = match reader.next_frame(&mut buffer) {
        Ok(info) => info,
        Err(error) => {
            crate::log::line(&format!("icon: cannot decode the embedded PNG: {error}"));
            return None;
        }
    };

    if info.bit_depth != png::BitDepth::Eight {
        crate::log::line("icon: only 8-bit PNG assets are supported");
        return None;
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
        color_type => {
            crate::log::line(&format!("icon: unsupported color type {color_type:?}"));
            return None;
        }
    };

    match Icon::from_rgba(rgba, info.width, info.height) {
        Ok(icon) => Some(icon),
        Err(error) => {
            crate::log::line(&format!("icon: cannot create the tray icon: {error}"));
            None
        }
    }
}

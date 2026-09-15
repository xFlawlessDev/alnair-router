//! Tray icon decoded from the shared `assets/alnair-white.ico` artwork.

use std::io::Cursor;

use ico::{IconDir, IconDirEntry};
use tray_icon::Icon;

use crate::error::{Error, Result};

/// App artwork shared with the installers and desktop shortcuts.
const ICO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/alnair-white.ico"
));

/// Native tray size: the Windows notification area or the macOS menu bar.
#[cfg(windows)]
const PREFERRED: u32 = 32;
#[cfg(target_os = "macos")]
const PREFERRED: u32 = 48;

/// Renders the tray icon from the app artwork.
pub(super) fn router() -> Result<Icon> {
    let (rgba, width, height) = bitmap()?;

    Icon::from_rgba(rgba, width, height)
        .map_err(|error| Error::Internal(format!("invalid tray icon bitmap: {error}")))
}

/// Decodes the ICO entry closest to [`PREFERRED`] into RGBA pixels.
fn bitmap() -> Result<(Vec<u8>, u32, u32)> {
    let dir = IconDir::read(Cursor::new(ICO))
        .map_err(|error| Error::Internal(format!("cannot read the tray icon: {error}")))?;
    let entry = pick(dir.entries())
        .ok_or_else(|| Error::Internal("the tray icon asset contains no images".to_string()))?;
    let image = entry
        .decode()
        .map_err(|error| Error::Internal(format!("cannot decode the tray icon: {error}")))?;

    Ok((image.rgba_data().to_vec(), image.width(), image.height()))
}

/// Smallest entry that is at least [`PREFERRED`] wide, else the largest one.
fn pick(entries: &[IconDirEntry]) -> Option<&IconDirEntry> {
    entries
        .iter()
        .filter(|entry| entry.width() >= PREFERRED)
        .min_by_key(|entry| entry.width())
        .or_else(|| entries.iter().max_by_key(|entry| entry.width()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_an_entry_at_least_native_size() {
        let (rgba, width, height) = bitmap().expect("decode the shipped asset");

        assert_eq!(width, height);
        assert!(width >= PREFERRED, "decoded {width}px icon");
        assert_eq!(rgba.len(), (width * height * 4) as usize);
    }

    #[test]
    fn picks_the_entry_closest_above_the_native_size() {
        let dir = IconDir::read(Cursor::new(ICO)).expect("read the shipped asset");
        let entry = pick(dir.entries()).expect("entry");

        assert!(entry.width() >= PREFERRED);
        assert!(
            dir.entries()
                .iter()
                .all(|other| other.width() < PREFERRED || other.width() >= entry.width()),
            "a smaller entry above {PREFERRED}px exists"
        );
    }
}

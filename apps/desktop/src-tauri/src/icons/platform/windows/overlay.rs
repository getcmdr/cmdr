//! The system's stock icons (`SHGetStockIconInfo`): the shortcut arrow composited onto a link's
//! icon, and the drive icons a volume row shows when its drive mustn't be asked.
//!
//! A stock icon is read from the icon location the Shell names (`imageres.dll` and friends, or
//! the override a user set under `Shell Icons`), at exactly the asked size. That location is
//! checked like any place (`place::may_read`), so an override pointing at `\\host\…` draws
//! nothing rather than reaching the network.

use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use cmdr_fs::ignore_poison::IgnorePoison;
use image::RgbaImage;
use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
use windows::Win32::UI::Shell::{
    SHDefExtractIconW, SHGSI_ICONLOCATION, SHGetStockIconInfo, SHSTOCKICONID, SHSTOCKICONINFO, SIID_DRIVECD,
    SIID_DRIVEFIXED, SIID_DRIVENET, SIID_DRIVERAM, SIID_DRIVEREMOVE, SIID_LINK,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
use windows::core::PCWSTR;

use crate::config::ICON_SIZE;

/// A drive kind's stock icon, for a volume row whose drive Cmdr doesn't ask (a share, a disc,
/// a removable drive: `DETAILS.md` § "Drive icons").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StockDriveIcon {
    Fixed,
    Removable,
    Optical,
    Network,
    Ram,
}

impl StockDriveIcon {
    fn id(self) -> SHSTOCKICONID {
        match self {
            Self::Fixed => SIID_DRIVEFIXED,
            Self::Removable => SIID_DRIVEREMOVE,
            Self::Optical => SIID_DRIVECD,
            Self::Network => SIID_DRIVENET,
            Self::Ram => SIID_DRIVERAM,
        }
    }
}

/// Each stock icon read so far, at `ICON_SIZE`. Only a successful read is kept, so a read that
/// failed (a thread the Shell wasn't ready on) is tried again next time.
static STOCK: LazyLock<Mutex<HashMap<i32, RgbaImage>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The stock icon `id` at `ICON_SIZE`, read once per run.
fn stock_icon(id: SHSTOCKICONID) -> Option<RgbaImage> {
    if let Some(img) = STOCK.lock_ignore_poison().get(&id.0) {
        return Some(img.clone());
    }
    let img = read_stock_icon(id, ICON_SIZE)?;
    STOCK.lock_ignore_poison().insert(id.0, img.clone());
    Some(img)
}

/// `kind`'s stock drive icon as a data URL, with no call to the drive.
pub(crate) fn stock_drive_icon(kind: StockDriveIcon) -> Option<String> {
    let img = stock_icon(kind.id())?;
    super::super::super::image_to_data_url(&image::DynamicImage::ImageRgba8(img))
}

/// `img` with the system's shortcut arrow drawn over it, or `img` as it was when the arrow
/// can't be read. The overlay is a full-size icon with the arrow in its corner, so it's drawn
/// over the whole image, as the Shell's image list does.
pub(super) fn with_link_arrow(mut img: RgbaImage) -> RgbaImage {
    if let Some(arrow) = stock_icon(SIID_LINK) {
        let arrow = super::shell_image::fit(arrow, img.width());
        if arrow.height() == img.height() {
            composite_over(&mut img, &arrow);
        }
    }
    img
}

/// Draws `top` over `base` (both straight-alpha RGBA, the same size), Porter-Duff "over".
pub(super) fn composite_over(base: &mut RgbaImage, top: &RgbaImage) {
    for (under, over) in base.pixels_mut().zip(top.pixels()) {
        let top_alpha = u32::from(over[3]);
        if top_alpha == 0 {
            continue;
        }
        let base_alpha = u32::from(under[3]);
        // Alpha scaled by 255 on both sides: out = top + base * (1 - top).
        let base_share = base_alpha * (255 - top_alpha);
        let out_alpha_255 = top_alpha * 255 + base_share;
        if out_alpha_255 == 0 {
            continue;
        }
        for channel in 0..3 {
            let mixed = u32::from(over[channel]) * top_alpha * 255 + u32::from(under[channel]) * base_share;
            under[channel] = u8::try_from((mixed + out_alpha_255 / 2) / out_alpha_255).unwrap_or(u8::MAX);
        }
        under[3] = u8::try_from((out_alpha_255 + 127) / 255).unwrap_or(u8::MAX);
    }
}

/// Reads stock icon `id` from the location the Shell names for it, at `size` × `size`.
fn read_stock_icon(id: SHSTOCKICONID, size: u32) -> Option<RgbaImage> {
    let mut info = SHSTOCKICONINFO {
        cbSize: u32::try_from(size_of::<SHSTOCKICONINFO>()).ok()?,
        ..Default::default()
    };
    // SAFETY: `info` is a live out-struct with its size set, as the API requires.
    unsafe { SHGetStockIconInfo(id, SHGSI_ICONLOCATION, &raw mut info) }.ok()?;
    let len = info.szPath.iter().position(|&unit| unit == 0)?;
    let location = PathBuf::from(String::from_utf16_lossy(&info.szPath[..len]));
    if !super::place::may_read(&location) {
        return None;
    }
    extract_icon(&location, info.iIcon, size)
}

/// The icon at `index` (a resource id when negative) in the icon file at `location` (`.ico`,
/// `.dll`, `.exe`), read straight from that file at `size` × `size`, with no Shell item and no
/// handler: the caller has vetted `location`, and nothing reads it a second time.
pub(super) fn extract_icon(location: &Path, index: i32, size: u32) -> Option<RgbaImage> {
    let wide: Vec<u16> = location.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut icon = HICON::default();
    // SAFETY: `wide` is NUL-terminated and outlives the call; the large-icon out-pointer is
    // live, and the small one isn't wanted. The low word of the size is the large icon's.
    let extracted = unsafe {
        SHDefExtractIconW(
            PCWSTR(wide.as_ptr()),
            index,
            0,
            Some(&raw mut icon),
            None,
            size & 0xFFFF,
        )
    };
    if extracted.is_err() || icon.is_invalid() {
        return None;
    }
    let img = icon_pixels(icon);
    // SAFETY: `SHDefExtractIconW` handed this call the icon, and nothing else holds it.
    let _ = unsafe { DestroyIcon(icon) };
    img.map(|img| super::shell_image::fit(img, size))
}

/// An icon's pixels as straight-alpha RGBA: its color bitmap, with the mask as alpha when the
/// color bitmap carries none (an old icon).
fn icon_pixels(icon: HICON) -> Option<RgbaImage> {
    let mut info = ICONINFO::default();
    // SAFETY: `icon` is live and `info` is a live out-struct; the call hands back two bitmaps
    // this function owns and deletes below.
    unsafe { GetIconInfo(icon, &raw mut info) }.ok()?;
    let color = (!info.hbmColor.is_invalid())
        .then(|| super::shell_image::read_bitmap(info.hbmColor))
        .flatten();
    let mask = (!info.hbmMask.is_invalid())
        .then(|| super::shell_image::read_bitmap(info.hbmMask))
        .flatten();
    for bitmap in [info.hbmColor, info.hbmMask] {
        if !bitmap.is_invalid() {
            // SAFETY: `GetIconInfo` gave this call the bitmap, and it's selected into no DC.
            let _ = unsafe { DeleteObject(HGDIOBJ::from(bitmap)) };
        }
    }
    let (width, height, bgra) = color?;
    let mask = mask.filter(|(w, h, _)| (*w, *h) == (width, height)).map(|(_, _, m)| m);
    RgbaImage::from_raw(width, height, icon_bgra_to_rgba(bgra, mask.as_deref()))
}

/// An icon's color bitmap (straight BGRA, the way icon resources store it) as RGBA. When no
/// pixel carries alpha, `mask` gives it (a white mask pixel is transparent), else it's opaque.
pub(super) fn icon_bgra_to_rgba(mut pixels: Vec<u8>, mask: Option<&[u8]>) -> Vec<u8> {
    let no_alpha = pixels.as_chunks::<4>().0.iter().all(|px| px[3] == 0);
    let mask_pixels = mask.map(|mask| mask.as_chunks::<4>().0);
    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px.swap(0, 2);
        if no_alpha {
            let transparent = mask_pixels.and_then(|m| m.get(index)).is_some_and(|m| m[0] != 0);
            px[3] = if transparent { 0 } else { 255 };
        }
    }
    pixels
}

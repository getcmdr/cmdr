//! Asking the Shell for a place's icon pixels: `IShellItemImageFactory::GetImage` with
//! `SIIGBF_ICONONLY`, read back through `GetDIBits`. Why this API and not `SHGetFileInfoW`:
//! `DETAILS.md` § "Asking Windows for the pixels".

use std::path::Path;

use image::RgbaImage;
use image::imageops::FilterType;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
use windows::core::HSTRING;

/// The Shell's icon for the place at native `path`, `size` × `size` straight-alpha RGBA, or
/// `None` when the Shell has none for it (gone, unreadable, no icon).
///
/// `SIIGBF_ICONONLY` keeps it to the icon: never a thumbnail, which would read the file's
/// content (and on a cloud placeholder, download it).
pub(super) fn render(path: &Path, size: u32) -> Option<RgbaImage> {
    super::apartment::ensure();
    let side = i32::try_from(size).ok()?;
    // SAFETY: the path is a live, NUL-terminated `HSTRING` for the whole call, and no bind
    // context is passed (`None`), which the API documents as allowed.
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None) }.ok()?;
    // SAFETY: `factory` is a live interface; the Shell hands back a bitmap this call owns.
    let bitmap = unsafe { factory.GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY) }.ok()?;
    let pixels = read_bitmap(bitmap);
    // SAFETY: `GetImage` gave this call the bitmap, nothing else holds it, and it isn't
    // selected into any DC (`read_bitmap` reads it through `GetDIBits`, which needs no select).
    let _ = unsafe { DeleteObject(HGDIOBJ::from(bitmap)) };
    let (width, height, bgra) = pixels?;
    let img = RgbaImage::from_raw(width, height, bgra_to_straight_rgba(bgra)?)?;
    Some(if (width, height) == (size, size) {
        img
    } else {
        image::imageops::resize(&img, size, size, FilterType::Lanczos3)
    })
}

/// The bitmap's pixels as 32-bit top-down BGRA, with its width and height.
fn read_bitmap(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut header = BITMAP::default();
    let header_len = i32::try_from(size_of::<BITMAP>()).ok()?;
    // SAFETY: `header` is a `BITMAP` of exactly `header_len` bytes, which `GetObjectW` fills.
    let written = unsafe { GetObjectW(HGDIOBJ::from(bitmap), header_len, Some((&raw mut header).cast())) };
    if written == 0 {
        return None;
    }
    let width = u32::try_from(header.bmWidth).ok().filter(|&w| w > 0)?;
    let height = header.bmHeight.unsigned_abs();
    if height == 0 {
        return None;
    }
    // SAFETY: a memory DC compatible with the screen; deleted below on every path.
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }
    let pixels = read_rows(dc, bitmap, width, height);
    // SAFETY: `dc` came from `CreateCompatibleDC` above and nothing else uses it.
    let _ = unsafe { DeleteDC(dc) };
    pixels.map(|bgra| (width, height, bgra))
}

/// `GetDIBits` into a 32-bit top-down buffer. `BI_RGB` at 32 bits writes no color table, so the
/// one-entry `BITMAPINFO` is big enough (`BI_BITFIELDS` would write three masks past it).
fn read_rows(dc: HDC, bitmap: HBITMAP, width: u32, height: u32) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
            biWidth: i32::try_from(width).ok()?,
            // Negative: top-down rows, the order `RgbaImage` reads.
            biHeight: -i32::try_from(height).ok()?,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buffer = vec![0u8; usize::try_from(width).ok()? * usize::try_from(height).ok()? * 4];
    // SAFETY: `buffer` holds `height` rows of `width` 32-bit pixels, exactly what the header
    // asks for, and `info` is a valid `BITMAPINFO` that `BI_RGB` won't write past.
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(buffer.as_mut_ptr().cast()),
            &raw mut info,
            DIB_RGB_COLORS,
        )
    };
    (u32::try_from(lines).ok() == Some(height)).then_some(buffer)
}

/// GDI's BGRA as straight-alpha RGBA. The Shell hands icons back premultiplied, so each color
/// is divided by its alpha; a buffer where some color exceeds its alpha can't be premultiplied
/// and is taken as straight already. An icon with no alpha at all (every alpha 0, an old
/// 24-bit icon) is opaque. A buffer of nothing but zero bytes is no icon at all (`None`), not
/// a black square: the Shell drew nothing.
pub(super) fn bgra_to_straight_rgba(mut pixels: Vec<u8>) -> Option<Vec<u8>> {
    if pixels.iter().all(|&byte| byte == 0) {
        return None;
    }
    let no_alpha = pixels.as_chunks::<4>().0.iter().all(|px| px[3] == 0);
    let premultiplied = !no_alpha
        && pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]);
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
        if no_alpha {
            px[3] = 255;
        } else if premultiplied && px[3] != 0 && px[3] != 255 {
            let alpha = u16::from(px[3]);
            for channel in &mut px[..3] {
                *channel = u8::try_from((u16::from(*channel) * 255 + alpha / 2) / alpha).unwrap_or(u8::MAX);
            }
        }
    }
    Some(pixels)
}

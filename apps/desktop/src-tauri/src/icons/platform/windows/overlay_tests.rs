//! The shortcut arrow and stock drive icons, against the real Shell on the Windows CI lane (the
//! pure halves run there too).

use std::path::Path;

use image::{Rgba, RgbaImage};

use super::tests::{explorer_exe, write_shortcut};
use super::{StockDriveIcon, fetch_path_icons, overlay, shell_image, shortcut, stock_drive_icon};
use crate::config::ICON_SIZE;
use crate::icons::get_icons;
use crate::test_support::TestDir;

fn app(path: &Path) -> String {
    crate::app_text::of_native(path).expect("app text")
}

/// The premise the composite rests on: the Shell's image of a link is its target's icon with
/// no arrow, so drawing the arrow ourselves never draws it twice.
#[test]
fn the_shells_image_of_a_link_has_no_arrow_and_ours_does() {
    let dir = TestDir::new("icon_link_arrow");
    let lnk = dir.as_ref().join("Explorer.lnk");
    write_shortcut(&lnk, &explorer_exe(), None);

    let plain_link = shell_image::render_plain(&lnk, ICON_SIZE).expect("the link's plain image");
    let target = shell_image::render_plain(&explorer_exe(), ICON_SIZE).expect("explorer.exe's image");
    assert_eq!(plain_link, target, "GetImage draws a link as its target, arrow-free");

    let with_arrow = shell_image::render(&lnk, ICON_SIZE).expect("the link's icon");
    assert_ne!(with_arrow, plain_link, "the link carries the shortcut arrow");
    assert_eq!(
        shell_image::render(&explorer_exe(), ICON_SIZE).as_ref(),
        Some(&target),
        "a program that isn't a link gets no arrow"
    );
}

/// The type icon of a shortcut, the stand-in for one Cmdr may not read, carries the arrow too.
#[test]
fn the_shortcut_type_icon_carries_the_arrow() {
    let sample = crate::icons::icon_sample_path("lnk").expect("a .lnk sample");
    let plain = shell_image::render_plain(&sample, ICON_SIZE).expect("the sample's plain image");
    let icon = shell_image::render(&sample, ICON_SIZE).expect("the sample's icon");
    assert_ne!(icon, plain);
}

/// A shortcut whose target is on a share is read without resolving and shows the generic
/// shortcut icon: the same pixels as the `.lnk` type's, arrow included.
#[test]
fn a_shortcut_to_a_unc_target_shows_the_generic_shortcut_icon() {
    let dir = TestDir::new("icon_link_unc");
    let lnk = dir.as_ref().join("OnShare.lnk");
    write_shortcut(&lnk, Path::new(r"\\cmdr-test.invalid\share\tool.exe"), None);
    assert!(!shortcut::points_only_here(&lnk));

    let own = fetch_path_icons(vec![app(&lnk)])
        .pop()
        .and_then(|(_, url)| url)
        .expect("an icon for the link");
    let generic = get_icons(vec!["ext:lnk".to_string()], false);
    assert_eq!(Some(&own), generic.get("ext:lnk"));
}

#[test]
fn every_stock_drive_icon_draws() {
    for kind in [
        StockDriveIcon::Fixed,
        StockDriveIcon::Removable,
        StockDriveIcon::Optical,
        StockDriveIcon::Network,
        StockDriveIcon::Ram,
    ] {
        assert!(stock_drive_icon(kind).is_some(), "{kind:?}");
    }
    assert_ne!(
        stock_drive_icon(StockDriveIcon::Network),
        stock_drive_icon(StockDriveIcon::Fixed)
    );
}

#[test]
fn an_opaque_overlay_pixel_replaces_and_a_clear_one_keeps() {
    let mut base = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 255]));
    let mut top = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 0]));
    top.put_pixel(0, 0, Rgba([200, 100, 50, 255]));
    overlay::composite_over(&mut base, &top);
    assert_eq!(base.get_pixel(0, 0), &Rgba([200, 100, 50, 255]));
    assert_eq!(base.get_pixel(1, 0), &Rgba([10, 20, 30, 255]));
}

#[test]
fn a_half_clear_overlay_over_a_clear_base_keeps_its_own_color() {
    let mut base = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0]));
    let top = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 128]));
    overlay::composite_over(&mut base, &top);
    assert_eq!(base.get_pixel(0, 0), &Rgba([200, 100, 50, 128]));
}

#[test]
fn an_icon_without_alpha_takes_it_from_its_mask() {
    // Two BGRA pixels with no alpha; the mask's first is white (transparent).
    let rgba = overlay::icon_bgra_to_rgba(vec![1, 2, 3, 0, 4, 5, 6, 0], Some(&[255, 255, 255, 0, 0, 0, 0, 0]));
    assert_eq!(rgba, vec![3, 2, 1, 0, 6, 5, 4, 255]);
    let straight = overlay::icon_bgra_to_rgba(vec![1, 2, 3, 128], None);
    assert_eq!(straight, vec![3, 2, 1, 128]);
}

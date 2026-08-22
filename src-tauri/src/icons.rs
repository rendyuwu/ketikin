//! Which raster the running app hands Windows, chosen for the size Windows is
//! about to draw it at.
//!
//! This module exists because of one Tauri detail and two Windows ones.
//!
//! Tauri's build-time codegen turns `icons/icon.ico` into a single RGBA buffer by
//! decoding `icon_dir.entries()[0]` and discarding the rest
//! ([tauri-apps/tauri#14596](https://github.com/tauri-apps/tauri/issues/14596)).
//! That buffer is `default_window_icon()`: the icon the window is created with and
//! the one `tray::artifact` hands the tray. So at runtime the app has exactly one
//! raster where the file has six. Which entry it is matters and is not arbitrary —
//! see `icons/README.md` — but no single raster can be right for every display
//! scale, which is the gap this module closes.
//!
//! Windows draws that raster at two families of size, both of which follow the
//! display scale:
//!
//! - the **small** icon, `SM_CXSMICON`: 16px at 100% scaling, 20 at 125%, 24 at
//!   150%, `16 * scale` in general. The titlebar and the notification area draw
//!   this one.
//! - the **large** icon, `SM_CXICON`: `32 * scale`. Alt+Tab and the taskbar button
//!   draw this one.
//!
//! `tao` only ever sets the small icon: `set_window_icon` passes `IconType::Small`,
//! `set_taskbar_icon` (`IconType::Big`) is never called by Tauri, and the window
//! class registers a null `hIcon`. Which surfaces then fall back to the small icon
//! rather than to the executable's resource group is not something this process can
//! observe, so [`window_pick`] assumes the worst and only ever replaces the window
//! icon with an entry that serves *both* families at least as well as the 64px
//! default already does.
//!
//! The tray carries no such doubt — its `HICON` is drawn in the notification area
//! and nowhere else — so [`tray_pick`] simply asks for the exact size, or the next
//! one up when the exact size was never drawn.
//!
//! Compiled on Windows, and under `cfg(test)` everywhere else, because the rules
//! below are arithmetic over embedded files and CI runs its tests on Linux.

#[cfg(windows)]
use std::sync::atomic::{AtomicU32, Ordering};

use tauri::image::Image;

/// The whole `.ico`, embedded so the five entries the codegen threw away can be
/// read back at runtime.
///
/// Roughly 8.6 KB, and it is the same file the executable's resource icon is built
/// from: decoding entries out of it here is what keeps this module from shipping
/// the same purpose-drawn rasters a second time as loose PNGs.
const APP_ICO: &[u8] = include_bytes!("../icons/icon.ico");

/// The run-state artifact at every size the notification area realistically asks
/// for: 100% through 300% scaling.
///
/// The idle side of this comes out of the `.ico`, which the run state has no entry
/// in — so unlike the idle rasters these are committed as loose PNGs, one render of
/// `icons/tray-run.svg` each. 32 is the file every non-macOS platform already
/// embeds through `tray::RUN_BYTES`; it is included here again rather than reached
/// across module and platform boundaries, because on macOS that constant is a
/// different drawing entirely.
///
/// Above 300% the largest of these is scaled up, so the run mark softens where the
/// idle icon would not. Covering 350% and beyond would mean mirroring the `.ico`'s
/// 64 and 256 entries for a mark two units wide; the sizes here are the ones
/// Windows offers for a display someone can actually read a 16px tray icon on.
const RUN_RASTERS: &[(u32, &[u8])] = &[
    (16, include_bytes!("../icons/tray-run-16.png")),
    (24, include_bytes!("../icons/tray-run-24.png")),
    (32, include_bytes!("../icons/tray-run.png")),
    (48, include_bytes!("../icons/tray-run-48.png")),
];

/// `SM_CXSMICON` at 100% scaling.
const SMALL_BASE: f64 = 16.0;

/// `SM_CXICON` at 100% scaling.
const LARGE_BASE: f64 = 32.0;

/// The largest scale factor treated as real, so float arithmetic from the OS
/// cannot walk off the end of the sizes here. Windows' own custom-scaling field
/// stops at 500%. A value that is not a number at all is refused outright rather
/// than clamped — see [`scaled`].
const MAX_SCALE: f64 = 5.0;

/// The size Windows draws the small icon at — the titlebar and the notification
/// area — for this scale factor. Zero when the scale factor is not a usable
/// number, which every caller reads as "change nothing".
pub fn small_icon_px(scale: f64) -> u32 {
    scaled(SMALL_BASE, scale)
}

/// The size Windows draws the large icon at — Alt+Tab, the taskbar button — for
/// this scale factor. Zero on the same terms as [`small_icon_px`].
pub fn large_icon_px(scale: f64) -> u32 {
    scaled(LARGE_BASE, scale)
}

fn scaled(base: f64, scale: f64) -> u32 {
    // NaN and both infinities mean the monitor could not be read rather than that
    // it is enormous, so they ask for nothing at all; a finite number outside the
    // range a display can be set to is clamped instead.
    if !scale.is_finite() {
        return 0;
    }

    (base * scale.clamp(1.0, MAX_SCALE)).round() as u32
}

/// Every PNG-encoded entry in the embedded `.ico`, in the order the file stores
/// them — so the first one is the entry the codegen decoded.
///
/// Hand-rolled because the `ico` crate is a build-time dependency of
/// `tauri-codegen`, not one of ours, and the fields this needs sit at fixed
/// offsets: a six-byte `ICONDIR` (reserved `0`, type `1` for an icon, then the
/// entry count) followed by 16-byte `ICONDIRENTRY`s whose width is byte 0 — `0`
/// meaning 256 — and whose payload is the last two `u32`s, byte count then offset.
/// `tray::tests` reads the same header the same way.
///
/// PNG-encoded only, and that is a filter rather than an assumption:
/// `Image::from_bytes` goes through `image::load_from_memory` with only the
/// `image-png` feature enabled, so a BMP/DIB entry — which is what most `.ico`
/// writers still emit below 64px — would not decode. An entry that cannot be
/// decoded must never win a pick, or the icon would quietly fall back to the
/// bundled artifact for the whole session.
fn entries() -> impl Iterator<Item = (u32, &'static [u8])> {
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
    const HEADER: usize = 6;
    const ENTRY: usize = 16;

    let count = if APP_ICO.len() >= HEADER && APP_ICO[..4] == [0, 0, 1, 0] {
        usize::from(u16::from_le_bytes([APP_ICO[4], APP_ICO[5]]))
    } else {
        0
    };

    (0..count).filter_map(move |index| {
        let start = HEADER + index * ENTRY;
        let entry = APP_ICO.get(start..start + ENTRY)?;
        // Square by construction — every raster in this repo is — so the height
        // byte carries nothing the width byte does not.
        let px = match entry[0] {
            0 => 256,
            width => u32::from(width),
        };
        let size = usize::try_from(u32::from_le_bytes(entry[8..12].try_into().ok()?)).ok()?;
        let offset = usize::try_from(u32::from_le_bytes(entry[12..16].try_into().ok()?)).ok()?;
        let payload = APP_ICO.get(offset..offset.checked_add(size)?)?;

        payload.starts_with(PNG_MAGIC).then_some((px, payload))
    })
}

/// The entry the build-time codegen turned into the runtime icon: the first one in
/// the file.
fn default_entry_px() -> Option<u32> {
    entries().next().map(|(px, _)| px)
}

/// Which `.ico` entry the window icon should be at this scale factor.
///
/// `None` only when the `.ico` cannot be read or the scale factor is nonsense;
/// otherwise this always names an entry, including the one already installed —
/// resolving to the default is how a move back down to 100% scaling gets the 64px
/// entry restored rather than keeping whatever a higher scale left behind.
pub fn window_entry_px(scale: f64) -> Option<u32> {
    let sizes: Vec<u32> = entries().map(|(px, _)| px).collect();
    let default_px = *sizes.first()?;

    window_pick(
        small_icon_px(scale),
        large_icon_px(scale),
        &sizes,
        default_px,
    )
}

/// The window rule, deliberately conservative.
///
/// An entry only wins if it is *at least* the large size Windows asks for — never
/// shrinking the buffer Alt+Tab and the taskbar may be scaling from, since whether
/// they fall back to `ICON_SMALL` is unobservable from here — and is an exact
/// integer multiple of the small size, so the titlebar's own downscale lands every
/// edge of this whole-unit drawing back on a pixel boundary. Anything else keeps
/// the entry the build installed.
///
/// Against this repo's 64/16/24/32/48/256 file that fires at 150% and nowhere else:
/// there the small icon is 24, the large one 48, and the 48px entry is both an
/// exact 2:1 for the titlebar and an exact match for Alt+Tab, where the 64px
/// default is a 2.67:1 resample of one and a 1.33:1 resample of the other. 100%
/// and 200% are already exact from the default (64/16 and 64/32, both whole
/// numbers), and at 125% and 175% no entry divides 20 or 28 at all — those sizes
/// are 1.25 and 1.75 units per pixel on a 16-unit grid, so no raster of this
/// drawing is crisp there and there is nothing to win.
fn window_pick(small: u32, large: u32, available: &[u32], default_px: u32) -> Option<u32> {
    if small == 0 || large == 0 {
        return None;
    }

    let serves = |px: u32| px >= large && px % small == 0;

    if serves(default_px) {
        return Some(default_px);
    }

    Some(
        available
            .iter()
            .copied()
            .filter(|&px| serves(px))
            .min()
            .unwrap_or(default_px),
    )
}

/// The tray rule: the exact size when it exists, the next size up when it does not,
/// and the largest raster on offer only when the ask is bigger than anything drawn.
///
/// No large-icon constraint, because a tray `HICON` is drawn in the notification
/// area and nowhere else. `None` only when the wanted size is unknown.
fn tray_pick(want: u32, available: &[u32]) -> Option<u32> {
    if want == 0 {
        return None;
    }

    available
        .iter()
        .copied()
        .filter(|&px| px >= want)
        .min()
        .or_else(|| available.iter().copied().max())
}

/// Decode a `.ico` entry, or `None` if the file has no entry that size.
pub fn app_icon(px: u32) -> Option<Image<'static>> {
    let (_, bytes) = entries().find(|&(size, _)| size == px)?;

    decode(px, bytes, "app icon")
}

/// The tray artifact for the size the notification area is asking for.
///
/// `None` when that size is not known — see [`tray_icon_px`], where zero means the
/// monitor could not be asked — so the caller falls back to the bundled artifact,
/// which is what every platform did before this module existed.
pub fn tray_icon(px: u32, running: bool) -> Option<Image<'static>> {
    if running {
        let sizes: Vec<u32> = RUN_RASTERS.iter().map(|&(size, _)| size).collect();
        let pick = tray_pick(px, &sizes)?;
        let (_, bytes) = RUN_RASTERS.iter().find(|&&(size, _)| size == pick)?;

        decode(pick, bytes, "run-state tray icon")
    } else {
        let sizes: Vec<u32> = entries().map(|(size, _)| size).collect();

        app_icon(tray_pick(px, &sizes)?)
    }
}

/// Decoded here rather than at build time because `Image` owns pixels, not a PNG.
/// The bytes are embedded, so this cannot fail on a missing file, only on a corrupt
/// one — and a corrupt one has to leave the icon that is showing alone rather than
/// take the app down over 24 pixels.
fn decode(px: u32, bytes: &[u8], what: &str) -> Option<Image<'static>> {
    match Image::from_bytes(bytes) {
        Ok(image) => Some(image),
        Err(err) => {
            log::warn!(
                "icons: the {px}px {what} will not decode ({err}); leaving the current icon in place"
            );
            None
        }
    }
}

/// The `.ico` entry the window is carrying right now, or `0` for "still whatever
/// the build-time codegen installed".
///
/// Runtime bookkeeping rather than arithmetic, which is why this and the three
/// functions below do not follow the rest of the module onto other platforms:
/// there is nothing here for a test to check.
#[cfg(windows)]
static WINDOW_ICON_PX: AtomicU32 = AtomicU32::new(0);

/// The small-icon size the notification area is asking for, or `0` for "not asked
/// yet".
///
/// Zero rather than 16 on purpose: every reader treats it as "leave the bundled
/// artifact alone", so a monitor query that fails degrades to exactly the
/// behaviour this module did not exist for, instead of pinning the tray to a 16px
/// raster on a 150% display.
#[cfg(windows)]
static TRAY_ICON_PX: AtomicU32 = AtomicU32::new(0);

/// The size of the entry the window icon is currently drawn from, resolving the
/// initial zero to whatever the build installed so a 100% display does not spend a
/// redundant `set_icon` at startup.
#[cfg(windows)]
pub fn window_icon_px() -> u32 {
    match WINDOW_ICON_PX.load(Ordering::Relaxed) {
        0 => default_entry_px().unwrap_or(0),
        px => px,
    }
}

/// Record what `set_icon` just installed. Called after it succeeds, never before:
/// a failed swap has to leave this reading what is actually on screen or the next
/// scale change would skip the retry.
#[cfg(windows)]
pub fn set_window_icon_px(px: u32) {
    WINDOW_ICON_PX.store(px, Ordering::Relaxed);
}

/// The size the tray artifact is being picked for. Read from the typing worker
/// through `tray::artifact`, so it is an atomic rather than anything the main
/// thread could be holding — see `AppState`'s locking rule.
#[cfg(windows)]
pub fn tray_icon_px() -> u32 {
    TRAY_ICON_PX.load(Ordering::Relaxed)
}

#[cfg(windows)]
pub fn set_tray_icon_px(px: u32) {
    TRAY_ICON_PX.store(px, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scale factors Windows' own display settings offer, and the two sizes
    /// each one asks for.
    const SCALES: &[(f64, u32, u32)] = &[
        (1.0, 16, 32),
        (1.25, 20, 40),
        (1.5, 24, 48),
        (1.75, 28, 56),
        (2.0, 32, 64),
        (2.25, 36, 72),
        (2.5, 40, 80),
        (3.0, 48, 96),
    ];

    fn sizes() -> Vec<u32> {
        entries().map(|(px, _)| px).collect()
    }

    #[test]
    fn both_icon_sizes_follow_the_display_scale() {
        for &(scale, small, large) in SCALES {
            assert_eq!(small_icon_px(scale), small, "small icon at {scale}");
            assert_eq!(large_icon_px(scale), large, "large icon at {scale}");
        }
    }

    /// A scale factor is a float from the OS feeding what amounts to a table
    /// lookup. Nonsense has to arrive as zero, which every caller reads as "change
    /// nothing", rather than as a size no raster exists for.
    #[test]
    fn a_nonsense_scale_factor_asks_for_no_size_at_all() {
        for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(small_icon_px(scale), 0, "small icon at {scale}");
            assert_eq!(large_icon_px(scale), 0, "large icon at {scale}");
        }

        // A finite number out of range is clamped rather than refused: it is still
        // an ask, and the nearest raster answers it better than the bundled
        // default would.
        assert_eq!(small_icon_px(-3.0), 16);
        assert_eq!(small_icon_px(0.5), 16);
        assert_eq!(small_icon_px(12.0), 80);
    }

    /// The whole design rests on what is inside `icon.ico`, and every rule above
    /// was written against this exact set. A regeneration that dropped a size — or
    /// wrote its small entries as DIBs, which `Image::from_bytes` cannot read —
    /// would leave the rules silently picking something else.
    #[test]
    fn the_embedded_ico_holds_the_sizes_the_rules_are_written_for() {
        assert_eq!(
            sizes(),
            [64, 16, 24, 32, 48, 256],
            "icon.ico's PNG entries, in file order; the first is the one the \
             build-time codegen decodes"
        );
        assert_eq!(
            default_entry_px(),
            Some(64),
            "the entry the window is created with, which every rule here treats as \
             the one to beat"
        );
    }

    #[test]
    fn every_ico_entry_decodes_at_the_size_it_claims() {
        for px in sizes() {
            let icon = app_icon(px).unwrap_or_else(|| panic!("the {px}px entry should decode"));
            assert_eq!((icon.width(), icon.height()), (px, px));
        }
    }

    /// The one configuration the window rule fires in, and the seven it stays out
    /// of. If a future `.ico` makes this list longer that is fine; if it makes 100%
    /// or 200% swap away from an entry that already halves exactly, the rule has
    /// been broken.
    #[test]
    fn the_window_icon_is_only_replaced_where_a_better_entry_exists() {
        let picks: Vec<Option<u32>> = SCALES
            .iter()
            .map(|&(scale, _, _)| window_entry_px(scale))
            .collect();

        assert_eq!(
            picks,
            [
                Some(64), // 100%: 16 and 32 both divide 64
                Some(64), // 125%: nothing divides 20
                Some(48), // 150%: exact 2:1 for 24, exact match for 48
                Some(64), // 175%: nothing divides 28
                Some(64), // 200%: 32 divides 64
                Some(64), // 225%: nothing divides 36
                Some(64), // 250%: nothing divides 40
                Some(64), // 300%: nothing divides 48
            ]
        );
    }

    /// The safety property behind the rule, over every scale a display can report:
    /// the pick is either the entry the build installed or one that is genuinely
    /// better on both surfaces. Nothing in between, and in particular never a
    /// raster smaller than the large icon Alt+Tab may be scaling from.
    #[test]
    fn the_window_pick_never_costs_the_large_icon_anything() {
        let default_px = default_entry_px().expect("the ico is readable");

        for step in 4..=20 {
            let scale = f64::from(step) / 4.0;
            let (small, large) = (small_icon_px(scale), large_icon_px(scale));
            let pick = window_entry_px(scale).expect("the ico is readable");

            assert!(
                pick == default_px || (pick >= large && pick % small == 0),
                "{pick}px at {scale} scaling is neither the default nor an \
                 improvement on both the {small}px and {large}px asks"
            );
        }
    }

    #[test]
    fn the_tray_takes_the_exact_size_or_the_next_one_up() {
        let available = [16, 24, 32, 48];

        assert_eq!(tray_pick(16, &available), Some(16));
        assert_eq!(tray_pick(20, &available), Some(24));
        assert_eq!(tray_pick(24, &available), Some(24));
        assert_eq!(tray_pick(28, &available), Some(32));
        assert_eq!(tray_pick(32, &available), Some(32));
        assert_eq!(tray_pick(40, &available), Some(48));
        // Past everything drawn, the largest raster is upscaled — the alternative
        // is no icon.
        assert_eq!(tray_pick(64, &available), Some(48));
        // Unknown size: the caller keeps the bundled artifact.
        assert_eq!(tray_pick(0, &available), None);
        assert_eq!(tray_pick(16, &[]), None);
    }

    /// Both tray states have to be drawn from the same size at every scale the run
    /// rasters cover, or the icon would appear to change identity when a run
    /// starts — which is the one thing `tray-run.svg` is drawn not to do.
    #[test]
    fn the_two_tray_states_are_the_same_size_at_every_covered_scale() {
        for &(scale, small, _) in SCALES {
            let px = small_icon_px(scale);
            assert_eq!(px, small);

            let idle = tray_icon(px, false).unwrap_or_else(|| panic!("idle at {scale}"));
            let run = tray_icon(px, true).unwrap_or_else(|| panic!("run state at {scale}"));

            assert_eq!(
                (run.width(), run.height()),
                (idle.width(), idle.height()),
                "the tray states differ in size at {scale} scaling"
            );
        }
    }

    #[test]
    fn an_unknown_tray_size_leaves_the_bundled_artifact_alone() {
        assert!(tray_icon(0, false).is_none());
        assert!(tray_icon(0, true).is_none());
    }

    /// Every run raster is the idle raster of the same size plus one 2x2-unit mark,
    /// and nothing else. Re-rendering either from the wrong source SVG, or at the
    /// wrong size, would leave the two states identical or the mark hanging off the
    /// keycap — and on Windows these four files are now what the tray actually
    /// draws, at four different display scales, so the 32px pair `tray::tests`
    /// checks is no longer the whole story.
    #[test]
    fn every_run_raster_is_its_idle_raster_plus_the_mark() {
        for &(px, _) in RUN_RASTERS {
            let run = tray_icon(px, true).unwrap_or_else(|| panic!("the {px}px run raster"));
            let idle = app_icon(px).unwrap_or_else(|| panic!("the {px}px ico entry"));

            assert_eq!(
                (run.width(), run.height()),
                (idle.width(), idle.height()),
                "the {px}px run raster is not {px}px"
            );

            // One eighth in is where the keycap starts on this 16-unit grid (2 of
            // 16). A mark that reaches the edge of the key reads as a nick taken
            // out of it rather than a mark on it.
            let margin = px / 8;
            let mut differing = 0;
            for (index, (marked, plain)) in
                run.rgba().chunks(4).zip(idle.rgba().chunks(4)).enumerate()
            {
                if marked == plain {
                    continue;
                }
                differing += 1;
                let (x, y) = (index as u32 % px, index as u32 / px);
                assert!(
                    x >= margin && x < px - margin && y >= margin && y < px - margin,
                    "the {px}px run mark reaches the edge of the key at ({x}, {y})"
                );
            }

            // 2x2 grid units is 4 pixels at 16px and more at every other size
            // here, so the floor is the smallest a conforming mark can ever be.
            assert!(
                differing >= 4,
                "the {px}px run mark is missing or too small: {differing} pixels differ"
            );
        }
    }
}

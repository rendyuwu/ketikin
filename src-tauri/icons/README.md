# Icons

Every raster here is a render of one of the two SVGs beside them, at its own size — not a resample
of a larger one. The reasoning for the drawing itself lives in `icon.svg` and `tray-run.svg`; read
those before moving a coordinate. This file covers the one thing about the *files* that is not
visible from looking at them: the order of the entries inside `icon.ico`.

## Inventory

| File | Used by |
| --- | --- |
| `icon.svg`, `tray-run.svg`, `tray-macos-template.svg`, `tray-macos-template-run.svg` | Sources. Nothing at build time reads them; the rasters are committed. |
| `icon.ico` | Windows. Both the executable's resource icon (Explorer, Start Menu, shortcuts) and — via its first entry only, see below — the runtime window and tray icon. |
| `icon.icns` | macOS bundle. |
| `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png` | The `bundle.icon` list in `tauri.conf.json`. The first PNG in that list is what `default_window_icon()` resolves to everywhere except Windows. |
| `Square*Logo.png`, `StoreLogo.png` | Windows Store assets, produced by `tauri icon` and kept for completeness. |
| `tray-run.png`, `tray-macos-template*.png` | Embedded in the binary by `src/tray.rs`, not bundled. |
| `icon-1024.png` | Source raster for `tauri icon` regeneration. |

## `icon.ico` must lead with its 64px entry

The file holds six entries — 16, 24, 32, 48, 64 and 256 — but they are stored **64 first**, then the
rest ascending. That order is load-bearing and looks arbitrary, so it is written down here.

Windows reads this file two different ways:

- **As a resource icon** (Explorer, the Start Menu, pinned shortcuts) it reads the whole group and
  picks the entry closest to the size it wants. Order is irrelevant on this path.
- **As the runtime icon** (the titlebar, and Ketikin's tray icon, which comes from
  `default_window_icon()`) it gets a single RGBA buffer that `tauri-codegen` decodes at build time.
  That code takes `icon_dir.entries()[0]` and throws the other five away —
  [tauri-apps/tauri#14596](https://github.com/tauri-apps/tauri/issues/14596). Whatever happens to be
  first in the file *is* the runtime icon at every display scaling.

The file used to lead with its 32px entry, so above 100% scaling the runtime icon was a 32px raster
being scaled up — the exact failure that redrawing the icon for small sizes ([#11]) existed to
remove. Leading with 64 means every size Windows asks for is reached by scaling *down* from a
purpose-drawn raster instead.

64 rather than 48, which is the other reasonable pick: the drawing is on a 16-unit grid with every
coordinate a whole unit, so 64 reaches 32 and 16 by exact halving and each edge lands back on a pixel
boundary. Those two sizes are what 100% scaling asks for, which is the common case and the one that
was already correct. 48 is an exact match at 150% but reaches 32 at 1.5:1, which would have made the
most common configuration softer to fix a rarer one.

Regenerating this file — `tauri icon`, ImageMagick, anything — will write the entries in its own
order and silently undo this. `tray::tests::the_ico_leads_with_the_entry_windows_decodes_at_runtime`
fails when that happens.

[#11]: https://github.com/rendyuwu/ketikin/issues/11

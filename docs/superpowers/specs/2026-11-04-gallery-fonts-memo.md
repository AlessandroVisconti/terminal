# Gallery fonts: memo for a future feature

Status: memo, 2026-11-04. Not a design and not scheduled; it records what was looked at when
the owner asked whether the theme gallery could also deliver suggested fonts, so that a design
can start from it. Builds on `2026-10-28-theme-gallery-design.md` (the catalog, credits,
install and update flow) and `2026-10-27-color-schemes-design.md` (schemes name a font family
and size).

## 1. The idea

Themes in the gallery often come from projects that recommend a monospace font (JetBrains Mono,
Fira Code, Cascadia Code, Iosevka and others). A gallery theme could name such a font, and
fbterm could download and use it, with its license and credit, the way it installs the theme
itself. Only fonts under permissive licenses, exactly as for themes.

## 2. What already fits

- fbterm loads its own fonts from bytes at startup (`FONTS` in `crates/fbterm/src/main.rs`:
  Geist, Geist Mono and the Lucide icon font), so a font file kept in the config dir can be
  loaded the same way and never needs installing system-wide. iced also offers runtime loading
  (`iced::font::load`), which is what an install would use.
- A scheme already carries `font_family` and `font_size`, so a gallery theme can name a
  gallery font by family and the install flow can fetch what the theme needs.
- The catalog (`catalog.toml` in `featherbitplatform/terminal-themes`, `fbterm-config/src/catalog.rs`),
  the `[credit]` block, the install hash, the "fetched only on a user action" rule, the
  "not part of fbterm, licenses stay with the files" stance and `net::get` all extend to fonts.
  The catalog would gain a `fonts` list: id, family name, version, download URL, size, SHA-256,
  license and credit, following the theme entries.

## 3. Licensing

- Nearly every popular terminal font is under the SIL Open Font License 1.1 or similar:
  JetBrains Mono, Fira Code, Cascadia Code, Iosevka, IBM Plex Mono, Source Code Pro, Geist Mono
  (all OFL), Hack (MIT plus the Bitstream Vera license). The OFL allows redistribution and
  bundling as long as the license file travels with the font and the font is not sold on its
  own. Nerd Font patched variants keep their base licenses.
- Not shippable, only nameable: SF Mono, Menlo, Monaco, Consolas and other OS fonts. A theme
  may still name them by family, as `font_family` already allows, and they apply only where
  the user has them.
- Only unmodified font files go in the catalog. The OFL Reserved Font Name rule applies as
  soon as a file is altered and still carries its name, so the catalog never patches fonts.
- Each font's license text is kept in `terminal-themes` next to the file (as `LICENSES.md`
  does for themes) and is written beside the installed font in the config dir.

## 4. What is different from themes

- **Size.** A theme file is about 1 KB (`MAX_FILE_BYTES` 64 KiB); a font family is 1 to 15 MB.
  Fonts are fetched one at a time, on demand, never with the catalog (`MAX_CATALOG_BYTES`
  512 KiB stays), with their own larger cap in `net::get`, a progress state in the window and a
  cancel. The catalog states each file's size so the prompt can show it.
- **Trust.** A font file is untrusted input parsed by the text stack. The SHA-256 pinned in the
  catalog, which the repository controls, is the main defence; the Rust font parsers are
  memory safe. A file whose hash does not match is deleted and reported, never loaded.
- **Activation.** A font installed while fbterm runs must be registered with iced at runtime
  and reach open panes that name it. Whether the terminal widget picks up a font loaded after
  startup without a restart has not been checked; it is the one technical unknown (section 6).
- **Export.** Profile archives and single-file scheme exports carry the font id and credit,
  never the bytes, so archives stay small and the license question stays with the catalog. An
  import on a machine without the font shows the theme with the global font and offers the
  download from the gallery.
- **Removal.** Deleting a gallery font while a scheme names it is allowed; those panes fall
  back to the global font, as they do today for any missing family.

## 5. Decisions for the owner

1. **Where fonts show.** Either a **Fonts** tab beside **Gallery** in the Color schemes window
   (browse, install, update, remove, with credit), or fonts that only ever arrive as part of a
   theme that names them (an install prompt: "This theme uses JetBrains Mono. Download it
   (3 MB)?"). The second is smaller and keeps the feature inside the existing flow; the first is
   what users will expect once they know fonts exist. A Fonts tab can come later on top of the
   prompt.
2. **Theme to font binding.** Whether a catalog theme may require a font (the install prompt
   above) or only suggest one (the theme installs with the global font; the credit line
   mentions the suggested font with a download button).
3. **Variants.** Which weights and styles to fetch: Regular and Bold only (what the terminal
   widget uses today), or the whole family. Regular and Bold keep downloads small.
4. **Ligatures and Nerd Font variants.** Whether patched fonts are in scope. They multiply the
   catalog and the licensing checks, and glyph coverage beyond what the widget renders today
   is a separate question.

## 6. Before any catalog work

A one-afternoon spike: load a font file from the config dir with `iced::font::load` while a
pane shows a scheme naming that family, and confirm the pane reflows to it without a restart,
on macOS, Linux and Windows. If it needs a restart, the install flow shows the same restart
note that the opacity setting shows today, and the design records it.

## 7. Rules that carry over unchanged

- No network access the user did not cause: the catalog, a theme and a font are fetched only
  on opening the Gallery, Refresh, Install, Update or an explicit download prompt. A remote
  program must never trigger any of it.
- Every font in the catalog has a permissive license and complete credit, shown wherever the
  font is shown, offline included.
- Fonts are not part of fbterm: nothing bundled beyond the three fonts fbterm ships today.

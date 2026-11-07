# svg

![The SVG page: a toolbar of three icon buttons (theme, scale, language), and five cards: gold inline star icons at four sizes, the same star.svg as images at 16 to 96 px, a card on a tiled SVG background, a "bevy SVG" badge drawn from SVG text, and an empty "Refused" card; the cards are framed by an SVG border-image](../screenshots/svg.png)

SVG in bevy_markup (ADR 0038): inline `<svg>`, `<img src="….svg">`, SVG
`border-image` and `background-image`, SVG `<text>`, and a hostile SVG
that's refused.

```sh
cargo run --example svg --features svg
cargo run --example svg --features svg-text   # with the text badge
```

## What you see

- **A toolbar of icon buttons.** Each icon is an inline `<svg>` with
  `fill="currentColor"`, so it takes the button's CSS color: hovering a
  button, or switching the theme, recolors it like text.
- **Inline icons** at 16, 24, 32 and 48 px, colored by the theme's accent.
- **One file, every size:** the same `star.svg` as an `<img>` at 16, 32,
  64 and 96 px. Each is drawn again at its displayed pixels, so the
  96 px star is as sharp as the 16 px one.
- **CSS images:** the cards are framed by `frame.svg` as a `border-image`
  (9-slice), and one card sits on a tiled `tile.svg` background.
- **Text:** `badge.svg` contains SVG `<text>`. With `svg-text` it's
  drawn with the fonts registered with bevy_markup. The example registers
  only system font families, which SVG text can't use, so the badge falls
  back to Bevy's default font. Without the feature, the text is blank.
- **Refused:** `refused.svg` is a `<use>` cycle that would overflow the
  SVG parser's stack. It's refused before parsing; nothing shows, and the
  page is unharmed.

The scale button steps `UiScale` through 1×, 1.5× and 2×: every SVG
(inline, `<img>`, frames, background) is drawn again at the new pixel
density with the same layout.

| Input | Action |
|---|---|
| Click a button | Theme / scale / language |
| Arrows / D-pad / left stick | Move between the buttons |
| Enter / gamepad A | Press the focused button |
| T / gamepad X | Next theme |
| S / gamepad B | Next `UiScale` (1×, 1.5×, 2×) |
| L / gamepad Y | Switch the language (English ↔ German) |

## What it shows

- **`currentColor` from the cascade:** an inline SVG's color is the
  element's computed CSS `color`, through `:hover` and context `var()`s
  (`--accent`, `--ink`).
- **Display-size redraws:** `<img>`, inline, `border-image` and
  `background-image` SVGs are drawn at the pixels they cover; frame
  corners and background tiles keep their logical size.
- **Untrusted SVG:** the loader's limits (no external files, a size cap,
  a recursion-depth bound, a render-cost budget) refuse hostile files
  without affecting the page.

## Files

| File | Contents |
|---|---|
| `main.rs` | The app: fonts, stylesheet, locales, the settings (theme, scale, language) and their hotkeys |
| `../assets/svg/page.html` | The template (the inline icons are in it) |
| `../assets/svg/style.css` | The stylesheet (theme `var()`s, the SVG `border-image` and `background-image`) |
| `../assets/svg/icons/star.svg`, `frame.svg`, `tile.svg`, `badge.svg`, `refused.svg` | The SVG files |
| `../assets/svg/locales/{en-US,de}/` | Fluent bundles |

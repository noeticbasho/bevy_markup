svg-title = SVG in bevy_markup
svg-theme = Theme: { $theme ->
    [0] Gold
    [1] Mint
   *[other] Rose
}
svg-scale = Scale: { $scale }×
svg-language = English
svg-inline-title = Inline icons
svg-inline-body = Inline &lt;svg&gt; in the template. Its <b>currentColor</b> is the CSS color: the theme and hover recolor it.
svg-sizes-title = One file, every size
svg-sizes-body = The same <b>star.svg</b> as an &lt;img&gt; at four CSS sizes, each drawn again at its displayed pixels.
svg-css-title = CSS images
svg-css-body = This card's background is a tiled SVG; the other cards are framed by an SVG border-image. Both stay sharp at any scale.
svg-text-title = Text
svg-text-body = SVG &lt;text&gt; needs the <b>svg-text</b> feature; it uses the fonts registered with bevy_markup, else Bevy's default font.
svg-refused-title = Refused
svg-refused-body = <b>refused.svg</b> is a &lt;use&gt; cycle that would crash the SVG parser: it's refused before parsing, so nothing shows here and the page is unharmed.
svg-status = UiScale { $scale } (T / X theme, S / B scale, L / Y language)

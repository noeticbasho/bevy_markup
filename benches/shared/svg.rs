//! The SVG benchmark documents and workloads (ADR 0038), shared by the
//! criterion (`svg`) and divan (`svg-divan`) targets.

#![allow(dead_code)]

/// A 24×24 icon: one path, `currentColor`.
pub(crate) const ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M12 2l3 7h7l-5.5 4.5 2 7.5-6.5-4.5-6.5 4.5 2-7.5L2 9h7z" fill="currentColor" stroke="#000" stroke-width="1"/></svg>"##;

/// A 512×512 illustration: 40 groups, each a gradient circle, a stroked
/// path and group opacity (an offscreen layer per group).
pub(crate) fn illustration() -> String {
    let mut body = String::from(
        r##"<defs><radialGradient id="g"><stop offset="0" stop-color="gold"/><stop offset="1" stop-color="#843"/></radialGradient></defs>"##,
    );
    for i in 0..40 {
        let (x, y) = (20 + (i % 8) * 60, 30 + (i / 8) * 95);
        body += &format!(
            r##"<g opacity="0.85"><circle cx="{x}" cy="{y}" r="26" fill="url(#g)"/><path d="M{x} {y} l20 30 l-40 0 z" fill="none" stroke="#222" stroke-width="3"/></g>"##
        );
    }
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512">{body}</svg>"#)
}

/// A 512×512 panel with a drop shadow (a blur filter over the layer).
pub(crate) const SHADOWED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><filter id="s"><feDropShadow dx="6" dy="6" stdDeviation="8"/></filter><rect x="32" y="32" width="448" height="448" rx="24" fill="#334" filter="url(#s)"/></svg>"##;

/// A chain of 30 patterns, each filled with the previous one: close to the
/// recursion-depth limit (the check's worst accepted case).
pub(crate) fn deep() -> String {
    let mut body = String::from(
        "<pattern id='p0' width='8' height='8' patternUnits='userSpaceOnUse'><rect width='8' height='8' fill='red'/></pattern>",
    );
    for i in 1..30 {
        body += &format!(
            "<pattern id='p{i}' width='8' height='8' patternUnits='userSpaceOnUse'><rect width='8' height='8' fill='url(#p{})'/></pattern>",
            i - 1
        );
    }
    body += "<rect width='64' height='64' fill='url(#p29)'/>";
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">{body}</svg>"#)
}

/// A 1920×1080 full-window background (ADR 0038's "full-window SVG
/// background at 4K": its 2× draw is 3840×2160): a gradient sky, a radial
/// sun, three layered ridges and six translucent clouds (an offscreen layer
/// each).
pub(crate) fn background() -> String {
    let mut body = String::from(
        r##"<defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1b2a4a"/><stop offset="1" stop-color="#f0a060"/></linearGradient><radialGradient id="sun"><stop offset="0" stop-color="#fff4c0"/><stop offset="1" stop-color="#ffb040" stop-opacity="0"/></radialGradient></defs><rect width="1920" height="1080" fill="url(#sky)"/><circle cx="1400" cy="640" r="260" fill="url(#sun)"/>"##,
    );
    for (i, color) in ["#40304a", "#2c2438", "#1a1624"].iter().enumerate() {
        let base = 700 + i * 120;
        body += &format!(
            r##"<path d="M0 {base} Q240 {} 480 {base} T960 {base} T1440 {base} T1920 {base} V1080 H0 Z" fill="{color}"/>"##,
            base - 160
        );
    }
    for i in 0..6 {
        let (x, y) = (150 + i * 300, 160 + (i % 3) * 70);
        body += &format!(
            r##"<g opacity="0.6"><ellipse cx="{x}" cy="{y}" rx="140" ry="40" fill="#fff"/><ellipse cx="{}" cy="{}" rx="90" ry="34" fill="#fff"/></g>"##,
            x + 60,
            y - 20
        );
    }
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080">{body}</svg>"#)
}

/// A 1024×1024 icon sheet (ADR 0038's "large icon sheet"): 32 × 32 cells
/// of 32 px, each a translated copy of the star path with its own fill and
/// a stroke.
pub(crate) fn sheet() -> String {
    let mut body = String::new();
    for i in 0..1024 {
        let (x, y) = ((i % 32) * 32 + 4, (i / 32) * 32 + 4);
        body += &format!(
            r##"<path transform="translate({x} {y})" d="M12 2l3 7h7l-5.5 4.5 2 7.5-6.5-4.5-6.5 4.5 2-7.5L2 9h7z" fill="rgb({}, {}, 120)" stroke="#000" stroke-width="1"/>"##,
            i % 256,
            (i * 7) % 256
        );
    }
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024">{body}</svg>"#)
}

/// The stage benchmarks' documents.
pub(crate) fn documents() -> Vec<(&'static str, String)> {
    vec![
        ("icon", ICON.to_owned()),
        ("illustration", illustration()),
        ("shadowed", SHADOWED.to_owned()),
        ("deep", deep()),
        ("background", background()),
        ("sheet", sheet()),
    ]
}

/// An inline `<svg>` page: `elements` icons, each in its own paragraph.
/// `recolor`: every paragraph's color follows `n` (and its index), so each
/// frame every icon is a new drawing (the cache's worst case); otherwise
/// they share one color and one drawing.
pub(crate) fn inline_page(elements: usize, recolor: bool) -> String {
    let icon = ICON.replace(r#" xmlns="http://www.w3.org/2000/svg""#, "");
    let color = if recolor {
        "rgb({{ (n + i) % 250 }}, 40, 40)"
    } else {
        "rgb(200, 40, 40)"
    };
    format!(
        r#"{{% for i in range(end={elements}) %}}<p style="color: {color}">Item {{{{ i }}}} {icon}</p>{{% endfor %}}"#
    )
}

/// An `<img>` page: `elements` icons from `icon.svg`, CSS-sized to 48 px
/// (drawn again at 48×48 by the display-size redraw).
pub(crate) fn img_page(elements: usize) -> String {
    format!(
        r#"{{% for i in range(end={elements}) %}}<p>Item {{{{ i }}}} <img class="icon" src="icon.svg"></p>{{% endfor %}}"#
    )
}

pub(crate) const CSS: &str = "html { color: #ffffff; font-size: 14px } img.icon { width: 48px }";

/// A frame workload's files.
pub(crate) fn files(page: String) -> Vec<(String, String)> {
    vec![
        ("page.html".to_owned(), page),
        ("style.css".to_owned(), CSS.to_owned()),
        ("icon.svg".to_owned(), ICON.to_owned()),
    ]
}

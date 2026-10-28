#!/usr/bin/env python3
"""Record a real browser's computed styles (and layout) for bevy_markup test vectors.

For each vector directory (default: every `tests/vectors/*/` holding
`page.html` + `style.css`) this builds a page with

    <style>* { all: unset }</style>   (no browser defaults)
    <style>{BEVY_MARKUP_CSS}</style>          (bevy_markup's own defaults and layout model)
    <style>{style.css}</style>
    <body>{page.html}<script>{collector}</script></body>

runs it in headless Chromium (`--dump-dom`, a VIEWPORT-sized window), and
writes `browser.json` next to the inputs: computed styles per element
(document order, `html` first) and the parent element's text style per
non-whitespace text node, as rendered (`text-transform` applied). For `layout_*` vectors each element also gets its
border box (`getBoundingClientRect`). The Rust tests `browser_oracle` and
`layout_oracle` in `tests/html_ui.rs` compare bevy_markup's output against it; no
browser is needed to run the tests.

Oracle vectors must be plain HTML (no Tera syntax, no `data-l10n-id`): the
browser sees the file as-is. Vectors with a `messages.ftl` belong to the
Fluent oracle (`scripts/fluent_oracle.sh`) and are skipped by default.
Layout vectors must not set `font-family` and must stay printable ASCII:
both engines then use the same font file, Bevy's embedded default font
(located through `cargo metadata`), which covers nothing else.

Usage:
    scripts/browser_oracle.py [--browser PATH] [VECTOR_DIR ...]

Only the Python standard library is required, plus cargo and a
Chromium/Chrome binary (found on PATH, or given with --browser /
$BEVY_MARKUP_BROWSER).
"""

import argparse
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VECTORS = ROOT / "tests" / "vectors"
BROWSERS = ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable", "chrome"]
# Headless Chromium widens narrower windows (500px was the minimum seen).
VIEWPORT = (640, 480)
DEFAULT_FONT = "FiraMono-subset.ttf"

# bevy_markup's defaults and layout model as CSS, between the reset and the vector's
# stylesheet. Keep in sync with `src/build.rs`: the `HtmlUi` root (the test
# spawns a full-width column) and containers are flex columns, so mixed
# inline content in them becomes one item per piece, as bevy_markup does; blocks are
# blocks; containers and blocks don't shrink (bevy_markup's `flex_shrink: 0`); text
# defaults to white in Bevy's default font at Bevy's default line height
# (1.2); `li` gets bevy_markup's indent and bullet text; `pre` keeps whitespace,
# doesn't wrap and has bevy_markup's 8px padding.
BEVY_MARKUP_CSS = """
@font-face { font-family: "bevy_markup default"; src: url("FiraMono-subset.ttf"); }
html { display: flex; flex-direction: column; color: #ffffff;
       font-family: "bevy_markup default"; font-size: 16px; line-height: 1.2; }
body, div, section, article, header, footer, main, nav, aside, ul, ol,
blockquote, figure, form, table, tbody, thead, tfoot, caption, td, th
{ display: flex; flex-direction: column; flex-shrink: 0; }
tr { display: flex; flex-direction: row; flex-shrink: 0; }
th { font-weight: bold; }
h1, h2, h3, h4, h5, h6, p, li, pre { display: block; flex-shrink: 0; }
li { margin-left: 12px; }
li::before { content: "\\2022  "; }
pre { white-space: pre; padding: 8px; }
head, script, style { display: none; }
"""

COLLECTOR = r"""
(() => {
  const self = document.currentScript;
  const px = (value) => parseFloat(value);
  const round = (value) => Math.round(value * 100) / 100;
  const text = (s) => ({
    color: s.color,
    fontSize: px(s.fontSize),
    fontWeight: Number(s.fontWeight),
    fontStyle: s.fontStyle,
    fontFamily: s.fontFamily,
  });
  const box = (s) => ({
    padding: [s.paddingTop, s.paddingRight, s.paddingBottom, s.paddingLeft].map(px),
    border: [s.borderTopWidth, s.borderRightWidth, s.borderBottomWidth, s.borderLeftWidth].map(px),
    borderImageSource: s.borderImageSource,
    borderImageSlice: s.borderImageSlice,
    borderImageRepeat: s.borderImageRepeat,
    rowGap: s.rowGap,
    backgroundColor: s.backgroundColor,
  });
  const record = (element) => {
    const s = getComputedStyle(element);
    const r = element.getBoundingClientRect();
    return {
      tag: element.localName,
      id: element.id || null,
      classes: [...element.classList],
      ...text(s),
      ...box(s),
      rect: [r.x, r.y, r.width, r.height].map(round),
    };
  };
  // Layout needs the web font; dumping waits for it (--virtual-time-budget).
  document.fonts.ready.then(() => {
    const elements = [record(document.documentElement)];
    const runs = [];
    const transformed = [];
    const visit = (parent) => {
      for (const node of parent.childNodes) {
        if (node === self) continue;
        if (node.nodeType === Node.ELEMENT_NODE) {
          elements.push(record(node));
          visit(node);
        } else if (node.nodeType === Node.TEXT_NODE && node.data.trim()) {
          const s = getComputedStyle(node.parentElement);
          if (s.textTransform !== "none") transformed.push([runs.length, node]);
          runs.push({ text: node.data.replace(/\s+/g, " "), ...text(s) });
        }
      }
    };
    visit(document.body);
    // text-transform changes the rendered text, not the DOM's: read each
    // node as Chrome renders it in its line. A selection over just that
    // node serializes the laid-out text, the transform applied in context
    // (capitalize continues a word across elements: `<b>foo</b>bar`).
    // An element's own innerText would transform it in isolation.
    const selection = window.getSelection();
    for (const [index, node] of transformed) {
      const range = document.createRange();
      range.selectNodeContents(node);
      selection.removeAllRanges();
      selection.addRange(range);
      runs[index].text = selection.toString().replace(/\s+/g, " ");
    }
    selection.removeAllRanges();
    const out = document.createElement("pre");
    out.id = "bevy_markup-oracle";
    out.textContent = JSON.stringify({ elements, runs });
    document.body.append(out);
  });
})();
"""


def default_font():
    """Bevy's embedded default font, from the bevy_text that Cargo.lock pins."""
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(ROOT / "Cargo.toml")],
        capture_output=True,
        text=True,
        check=True,
    )
    for package in json.loads(result.stdout)["packages"]:
        if package["name"] == "bevy_text":
            return Path(package["manifest_path"]).parent / "src" / DEFAULT_FONT
    sys.exit("bevy_text not found in `cargo metadata`")


def find_browser(explicit):
    for candidate in [explicit, os.environ.get("BEVY_MARKUP_BROWSER"), *BROWSERS]:
        if candidate and (path := shutil.which(candidate)):
            return path
    sys.exit("no Chromium/Chrome found; pass --browser PATH or set BEVY_MARKUP_BROWSER")


def browser_version(browser):
    result = subprocess.run([browser, "--version"], capture_output=True, text=True, check=True)
    return result.stdout.strip()


def record(browser, vector, font):
    page = (vector / "page.html").read_text(encoding="utf-8")
    css = (vector / "style.css").read_text(encoding="utf-8")
    if re.search(r"\{\{|\{%|data-l10n-id", page):
        sys.exit(f"{vector}: oracle vectors must be plain HTML (no Tera, no data-l10n-id)")
    layout = vector.resolve().name.startswith("layout_")
    if layout and ("font-family" in css or not (page + css).isascii()):
        sys.exit(f"{vector}: layout vectors use only the default font: no font-family, ASCII only")

    document = (
        "<!doctype html><html><head><meta charset=\"utf-8\">"
        f"<style>* {{ all: unset; }}</style><style>{BEVY_MARKUP_CSS}</style>"
        f"<style>{css}</style></head><body>{page}"
        f"<script>{COLLECTOR}</script></body></html>"
    )
    with tempfile.TemporaryDirectory(prefix="bevy_markup-oracle-") as tmp:
        harness = Path(tmp) / "index.html"
        harness.write_text(document, encoding="utf-8")
        shutil.copy(font, Path(tmp) / DEFAULT_FONT)
        result = subprocess.run(
            [
                browser,
                "--headless=new",
                "--disable-gpu",
                "--no-sandbox",
                "--hide-scrollbars",
                "--force-device-scale-factor=1",
                f"--window-size={VIEWPORT[0]},{VIEWPORT[1]}",
                "--virtual-time-budget=5000",
                "--dump-dom",
                harness.as_uri(),
            ],
            capture_output=True,
            text=True,
            timeout=60,
            check=True,
        )
        base = Path(tmp).as_uri() + "/"
    match = re.search(r'<pre id="bevy_markup-oracle">(.*?)</pre>', result.stdout, re.S)
    if not match:
        sys.exit(f"{vector}: collector output missing; browser stderr:\n{result.stderr}")
    data = json.loads(html.unescape(match.group(1)))
    # Computed url()s are absolute; the temp directory differs per run.
    for element in data["elements"]:
        element["borderImageSource"] = element["borderImageSource"].replace(base, "")
        if not layout:
            # Only layout vectors pin the font, so other rects aren't stable.
            del element["rect"]
    return data


def to_json(generator, data):
    """One record per line, so browser changes show up as readable diffs."""
    lines = ["{", f' "generator": {json.dumps(generator)},']
    for key in ("elements", "runs"):
        records = [f"  {json.dumps(item, ensure_ascii=False)}" for item in data[key]]
        comma = "," if key == "elements" else ""
        lines += [f' "{key}": [', ",\n".join(records), f" ]{comma}"]
    lines.append("}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--browser", help="Chromium/Chrome binary")
    parser.add_argument("vectors", nargs="*", type=Path, help="vector directories")
    args = parser.parse_args()

    browser = find_browser(args.browser)
    version = browser_version(browser)
    font = default_font()
    vectors = args.vectors or sorted(
        path.parent
        for path in VECTORS.glob("*/style.css")
        if (path.parent / "page.html").exists() and not (path.parent / "messages.ftl").exists()
    )
    for vector in vectors:
        data = record(browser, vector, font)
        generator = {"browser": version, "script": "scripts/browser_oracle.py", "viewport": VIEWPORT}
        (vector / "browser.json").write_text(to_json(generator, data), encoding="utf-8")
        print(f"{vector.resolve().relative_to(ROOT)}: {len(data['elements'])} elements, {len(data['runs'])} text runs")


if __name__ == "__main__":
    main()

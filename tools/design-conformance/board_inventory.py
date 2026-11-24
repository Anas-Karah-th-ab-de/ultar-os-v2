#!/usr/bin/env python3
"""Design conformance: render every artboard headless and extract its inventory.

For each `design/boards/project/*.dc.html` this script
  * renders the board at its own size (1x) to `<out>/boards/<Board>.png`,
  * extracts every interactive element (`data-act`), every icon (`data-icon`), every
    landmark/role element and every visible text run with its box, colours and font,
  * writes `<out>/inventory.json` (one entry per board).

Usage: board_inventory.py <repo-root> <out-dir> [--chrome /usr/bin/google-chrome] [--no-png]
Needs: python3 + playwright (pip) and a Chrome/Chromium binary. No network needed: remote
font links are blocked and the Noto fonts from `assets/fonts` (if present) are injected.
"""
import argparse
import json
import pathlib
import sys

EXTRACT_JS = r"""
() => {
  const root = document.querySelector('x-dc > div[dir], body > div[dir], div[dir]');
  const rb = root.getBoundingClientRect();
  const rel = (r) => ({x: Math.round((r.x - rb.x) * 100) / 100, y: Math.round((r.y - rb.y) * 100) / 100,
                       w: Math.round(r.width * 100) / 100, h: Math.round(r.height * 100) / 100});
  const name = (el) => {
    if (el.getAttribute('aria-label')) return el.getAttribute('aria-label');
    const lb = el.getAttribute('aria-labelledby');
    if (lb) { const t = document.getElementById(lb); if (t) return t.textContent.trim(); }
    if (el.labels && el.labels.length) return el.labels[0].textContent.replace(/\s+/g, ' ').trim();
    if (el.tagName === 'INPUT') return el.getAttribute('placeholder') || el.value || '';
    return (el.textContent || '').replace(/\s+/g, ' ').trim();
  };
  const style = (el) => {
    const s = getComputedStyle(el);
    return {bg: s.backgroundColor, fg: s.color, border: s.borderTopColor, bw: s.borderTopWidth,
            radius: s.borderTopLeftRadius, font: s.fontFamily, size: s.fontSize, weight: s.fontWeight,
            opacity: s.opacity, dir: s.direction, outline: s.outlineStyle === 'none' ? '' : (s.outlineColor + ' ' + s.outlineWidth + ' ' + s.outlineOffset),
            shadow: s.boxShadow, pad: s.paddingInlineStart};
  };
  const vis = (el) => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
  const painted = (el) => { const s = getComputedStyle(el);
    return s.backgroundColor !== 'rgba(0, 0, 0, 0)' || (parseFloat(s.borderTopWidth) > 0 && s.borderTopColor !== 'rgba(0, 0, 0, 0)'); };
  const visual = (el) => {
    let v = [el, ...el.querySelectorAll('*')].find(e => painted(e) && vis(e) && e.tagName !== 'svg' && !e.closest('svg'));
    if (!v && el.tagName === 'INPUT') { v = [el.parentElement, el.parentElement && el.parentElement.parentElement].find(e => e && painted(e)); }
    if (!v) return null;
    return {box: rel(v.getBoundingClientRect()), style: style(v), self: v === el};
  };
  const acts = [...document.querySelectorAll('[data-act]')].map((el, i) => ({
    i: i + 1, act: el.getAttribute('data-act'), tag: el.tagName.toLowerCase(), role: el.getAttribute('role'),
    name: name(el), disabled: el.disabled === true || el.getAttribute('aria-disabled') === 'true',
    pressed: el.getAttribute('aria-pressed'), checked: el.getAttribute('aria-checked') || (el.checked ? 'true' : null),
    icon: (el.querySelector('[data-icon]') || {getAttribute: () => null}).getAttribute('data-icon'),
    box: rel(el.getBoundingClientRect()), style: style(el), visible: vis(el), visual: visual(el)}));
  const icons = [...document.querySelectorAll('[data-icon]')].map(el => ({
    icon: el.getAttribute('data-icon'), size: el.getAttribute('width'), box: rel(el.getBoundingClientRect()),
    fg: getComputedStyle(el).color}));
  const regions = [...document.querySelectorAll('[role],[aria-label],main,nav,header,footer,section,aside,ul,dialog')]
    .filter(el => !el.hasAttribute('data-act')).map(el => ({
      tag: el.tagName.toLowerCase(), role: el.getAttribute('role'), name: el.getAttribute('aria-label') || '',
      box: rel(el.getBoundingClientRect()), style: style(el)}));
  const texts = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    const n = walker.currentNode; const t = n.textContent.replace(/\s+/g, ' ').trim();
    if (!t) continue; const p = n.parentElement; if (!vis(p)) continue;
    const range = document.createRange(); range.selectNodeContents(n);
    const s = getComputedStyle(p);
    texts.push({text: t, box: rel(range.getBoundingClientRect()), fg: s.color, size: s.fontSize, weight: s.fontWeight,
                lang_ltr_island: !!p.closest('bdi[dir=ltr],[dir=ltr]')});
  }
  const colors = new Set();
  for (const el of root.querySelectorAll('*')) {
    const s = getComputedStyle(el);
    for (const c of [s.backgroundColor, s.color, s.borderTopColor]) colors.add(c);
  }
  return {title: document.title, dir: root.getAttribute('dir'), lang: document.documentElement.lang,
          width: Math.round(rb.width), height: Math.round(rb.height), acts, icons, regions, texts,
          colors: [...colors]};
}
"""


def font_css(repo: pathlib.Path) -> str:
    """@font-face rules for the repo's own Noto fonts so the render needs no network."""
    rules = []
    fonts = sorted(repo.glob("assets/fonts/**/*.ttf")) + sorted(repo.glob("platform/ultar-ui/**/fonts/**/*.ttf"))
    for f in fonts:
        stem = f.stem
        fam = None
        for cand in ("Noto Naskh Arabic", "Noto Kufi Arabic", "Noto Sans Arabic", "Noto Sans Mono", "Noto Sans"):
            if stem.split("-")[0] == cand.replace(" ", ""):
                fam = cand
                break
        if not fam:
            continue
        weight = 400
        for w, key in ((700, "Bold"), (600, "SemiBold"), (500, "Medium")):
            if key in stem:
                weight = w
                break
        rules.append(f"@font-face{{font-family:'{fam}';src:url('file://{f}');font-weight:{weight}}}")
    return "\n".join(rules)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("repo")
    ap.add_argument("out")
    ap.add_argument("--chrome", default="/usr/bin/google-chrome")
    ap.add_argument("--no-png", action="store_true")
    a = ap.parse_args()
    repo = pathlib.Path(a.repo).resolve()
    out = pathlib.Path(a.out).resolve()
    (out / "boards").mkdir(parents=True, exist_ok=True)
    from playwright.sync_api import sync_playwright

    css = font_css(repo)
    inv = {}
    with sync_playwright() as p:
        br = p.chromium.launch(executable_path=a.chrome, args=["--font-render-hinting=none"])
        for f in sorted((repo / "design/boards/project").glob("*.dc.html")):
            board = f.name.split(".")[0]
            pg = br.new_page(viewport={"width": 2600, "height": 1300}, device_scale_factor=1)
            pg.route("**/*", lambda r: r.abort() if r.request.url.startswith("http") else r.continue_())
            pg.goto(f.as_uri())
            if css:
                pg.add_style_tag(content=css)
            pg.wait_for_timeout(300)
            data = pg.evaluate(EXTRACT_JS)
            data["file"] = str(f.relative_to(repo))
            inv[board] = data
            if not a.no_png:
                pg.locator("div[dir]").first.screenshot(path=str(out / "boards" / f"{board}.png"))
            pg.close()
        br.close()
    (out / "inventory.json").write_text(json.dumps(inv, ensure_ascii=False, indent=1))
    total = sum(len(b["acts"]) for b in inv.values())
    print(f"boards={len(inv)} clickable={total}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

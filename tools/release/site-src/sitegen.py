#!/usr/bin/env python3
"""Website generator for ultar.code-craft.tech.

Builds static pages from pages.py in three languages: Arabic at /, English at /en/, German at /de/.
Output: tools/release/site/ (deployed unchanged with tools/release/vps/deploy-web.sh).
No inline scripts or styles (the site's CSP allows only 'self'); dynamic parts (release lists, the
newest network update) are filled by /app.js from /downloads/releases.json and /v1/update/latest.

    python3 tools/release/site-src/sitegen.py [out-dir]
"""
import html
import re
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pages as P  # noqa: E402

SITE = "https://ultar.code-craft.tech"
LANGS = ("ar", "en", "de")
PREFIX = {"ar": "", "en": "/en", "de": "/de"}
ASSET_PATHS = ("/downloads/", "/img/", "/store/", "/v1/", "/style.css", "/app.js")
VERSION = P.ASSET_VERSION


def tx(v, lang):
    """Text of a value in a language (dicts per language, plain strings for all)."""
    if v is None:
        return ""
    if isinstance(v, dict):
        return v.get(lang) or v.get("en") or ""
    return str(v)


def loc(href, lang):
    """Internal page links get the language prefix; files and external links stay."""
    if href.startswith("/") and not href.startswith(ASSET_PATHS):
        return PREFIX[lang] + href
    return href


def ext(href):
    return href.startswith("http") or href.startswith("mailto:")


def inline(text, lang):
    """Escape, then a tiny markup: [label](href), **bold**, `code`."""
    s = html.escape(text, quote=False)
    s = re.sub(r"`([^`]+)`", lambda m: f'<code dir="ltr">{m.group(1)}</code>', s)
    s = re.sub(r"\*\*([^*]+)\*\*", r"<b>\1</b>", s)

    def link(m):
        href = m.group(2)
        rel = ' rel="noopener noreferrer"' if ext(href) else ""
        return f'<a href="{html.escape(loc(href, lang))}"{rel}>{m.group(1)}</a>'

    return re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", link, s)


def a(href, label, lang, cls=None, extra=""):
    c = f' class="{cls}"' if cls else ""
    rel = ' rel="noopener noreferrer"' if ext(href) else ""
    return f'<a{c} href="{html.escape(loc(href, lang))}"{rel}{extra}>{label}</a>'


def btns(items, lang):
    out = []
    for label, href, kind in items:
        cls = "btn primary" if kind == "primary" else "btn"
        out.append(a(href, html.escape(tx(label, lang)), lang, cls))
    return f'<div class="cta">{"".join(out)}</div>'


def shot(name, lang):
    """Screenshots exist in Arabic and English; German pages use the English ones."""
    v = "ar" if lang == "ar" else "en"
    return f"/img/shots/{name}-{v}.webp"


# ---------------------------------------------------------------- blocks

def b_hero(d, lang):
    parts = ['<section class="hero"><div class="hero-in">', '<div class="hero-tx">']
    if d.get("badge"):
        parts.append(f'<p class="badge">{inline(tx(d["badge"], lang), lang)}</p>')
    parts.append(f'<h1>{inline(tx(d["h1"], lang), lang)}</h1>')
    if d.get("lead"):
        parts.append(f'<p class="lead">{inline(tx(d["lead"], lang), lang)}</p>')
    if d.get("btns"):
        parts.append(btns(d["btns"], lang))
    if d.get("latest"):
        parts.append(f'<p class="latest" data-latest>{html.escape(tx(P.T["latest_wait"], lang))}</p>')
    parts.append("</div>")
    if d.get("img"):
        parts.append(f'<figure class="hero-img"><img src="{shot(d["img"], lang)}" alt="{html.escape(tx(d.get("alt"), lang))}" width="1440" height="900"></figure>')
    parts.append("</div></section>")
    return "".join(parts)


def b_sec(d, lang):
    cls = "band" + (" alt" if d.get("alt") else "")
    sid = f' id="{d["id"]}"' if d.get("id") else ""
    out = [f'<section class="{cls}"{sid}><div class="wrap">']
    if d.get("h"):
        out.append(f'<h2>{inline(tx(d["h"], lang), lang)}</h2>')
    if d.get("lead"):
        out.append(f'<p class="lead">{inline(tx(d["lead"], lang), lang)}</p>')
    for blk in d.get("body", []):
        out.append(render(blk, lang))
    out.append("</div></section>")
    return "".join(out)


def b_cards(items, lang, cols=3):
    out = [f'<div class="cards c{cols}">']
    for it in items:
        tag = "article"
        inner = []
        if it.get("k"):
            inner.append(f'<p class="kicker">{inline(tx(it["k"], lang), lang)}</p>')
        inner.append(f'<h3>{inline(tx(it["h"], lang), lang)}</h3>')
        if it.get("p"):
            inner.append(f'<p>{inline(tx(it["p"], lang), lang)}</p>')
        if it.get("href"):
            more = tx(it.get("more") or P.T["more"], lang)
            inner.append(a(it["href"], html.escape(more) + ' <span aria-hidden="true" class="arr">›</span>', lang, "more"))
        out.append(f'<{tag} class="card">{"".join(inner)}</{tag}>')
    out.append("</div>")
    return "".join(out)


def b_checks(items, lang, kind="ok"):
    lis = "".join(f"<li>{inline(tx(i, lang), lang)}</li>" for i in items)
    return f'<ul class="checks {kind}">{lis}</ul>'


def b_status(d, lang):
    cols = []
    for key, kind in (("ok", "ok"), ("new", "new"), ("no", "no")):
        if d.get(key):
            cols.append(f'<div class="col"><h3>{html.escape(tx(P.T["st_" + key], lang))}</h3>{b_checks(d[key], lang, kind)}</div>')
    return f'<div class="cols">{"".join(cols)}</div>'


def b_steps(items, lang):
    out = ['<ol class="steps">']
    for it in items:
        li = [f'<h3>{inline(tx(it["h"], lang), lang)}</h3>']
        if it.get("p"):
            li.append(f'<p>{inline(tx(it["p"], lang), lang)}</p>')
        if it.get("list"):
            li.append(b_checks(it["list"], lang, "dot"))
        if it.get("olist"):
            li.append('<ol class="olist">' + "".join(f"<li>{inline(tx(x, lang), lang)}</li>" for x in it["olist"]) + "</ol>")
        if it.get("code"):
            li.append(b_code(it["code"], lang))
        out.append(f'<li>{"".join(li)}</li>')
    out.append("</ol>")
    return "".join(out)


def b_code(text, lang):
    return f'<pre dir="ltr"><code>{html.escape(tx(text, lang))}</code></pre>'


def b_table(head, rows, lang):
    th = "".join(f"<th>{inline(tx(h, lang), lang)}</th>" for h in head)
    trs = "".join("<tr>" + "".join(f"<td>{inline(tx(c, lang), lang)}</td>" for c in r) + "</tr>" for r in rows)
    return f'<div class="tbl"><table><thead><tr>{th}</tr></thead><tbody>{trs}</tbody></table></div>'


def b_rel(kind, lang):
    return (f'<div class="releases" data-releases="{kind}">'
            f'<p class="muted">{inline(tx(P.T["rel_wait"], lang), lang)}</p></div>')


def b_shots(items, lang):
    figs = []
    for name, cap in items:
        figs.append(f'<figure><img src="{shot(name, lang)}" alt="{html.escape(tx(cap, lang))}" loading="lazy" width="1440" height="900">'
                    f'<figcaption>{inline(tx(cap, lang), lang)}</figcaption></figure>')
    return f'<div class="strip">{"".join(figs)}</div><p class="muted small">{html.escape(tx(P.T["shots_note"], lang))}</p>'


def b_faq(items, lang):
    out = []
    for q, ans in items:
        out.append(f'<details class="faq"><summary>{inline(tx(q, lang), lang)}</summary><p>{inline(tx(ans, lang), lang)}</p></details>')
    return "".join(out)


def b_news(items, lang, limit=None):
    out = ['<div class="news">']
    for it in items[:limit] if limit else items:
        out.append(f'<article class="card news-item"><p class="kicker"><time datetime="{it["date"]}">{it["date"]}</time></p>'
                   f'<h3>{inline(tx(it["h"], lang), lang)}</h3><p>{inline(tx(it["p"], lang), lang)}</p></article>')
    out.append("</div>")
    return "".join(out)


def b_split(d, lang):
    img = (f'<figure class="split-img"><img src="{shot(d["img"], lang)}" alt="{html.escape(tx(d.get("alt") or d["h"], lang))}" loading="lazy" width="1440" height="900"></figure>'
           if d.get("img") else "")
    body = [f'<h2>{inline(tx(d["h"], lang), lang)}</h2>']
    for p in d.get("ps", []):
        body.append(f"<p>{inline(tx(p, lang), lang)}</p>")
    if d.get("list"):
        body.append(b_checks(d["list"], lang, "dot"))
    if d.get("btns"):
        body.append(btns(d["btns"], lang))
    rev = " rev" if d.get("rev") else ""
    return f'<div class="split{rev}"><div class="split-tx">{"".join(body)}</div>{img}</div>'


def b_upd(lang):
    return (f'<div class="card upd" data-update><p class="kicker">{html.escape(tx(P.T["upd_k"], lang))}</p>'
            f'<p class="upd-v" data-update-v>{html.escape(tx(P.T["upd_wait"], lang))}</p></div>')


def b_kv(items, lang):
    dl = "".join(f"<div><dt>{inline(tx(k, lang), lang)}</dt><dd>{inline(tx(v, lang), lang)}</dd></div>" for k, v in items)
    return f'<dl class="kv">{dl}</dl>'


def render(blk, lang):
    kind = blk[0]
    if kind == "hero":
        return b_hero(blk[1], lang)
    if kind == "sec":
        return b_sec(blk[1], lang)
    if kind == "cards":
        return b_cards(blk[1], lang, blk[2] if len(blk) > 2 else 3)
    if kind == "checks":
        return b_checks(blk[1], lang, blk[2] if len(blk) > 2 else "ok")
    if kind == "status":
        return b_status(blk[1], lang)
    if kind == "steps":
        return b_steps(blk[1], lang)
    if kind == "code":
        return b_code(blk[1], lang)
    if kind == "table":
        return b_table(blk[1], blk[2], lang)
    if kind == "rel":
        return b_rel(blk[1], lang)
    if kind == "shots":
        return b_shots(blk[1], lang)
    if kind == "faq":
        return b_faq(blk[1], lang)
    if kind == "p":
        return f'<p>{inline(tx(blk[1], lang), lang)}</p>'
    if kind == "h3":
        return f'<h3 class="sub">{inline(tx(blk[1], lang), lang)}</h3>'
    if kind == "note":
        return f'<p class="note">{inline(tx(blk[1], lang), lang)}</p>'
    if kind == "btns":
        return btns(blk[1], lang)
    if kind == "news":
        return b_news(P.NEWS, lang, blk[1] if len(blk) > 1 else None)
    if kind == "split":
        return b_split(blk[1], lang)
    if kind == "upd":
        return b_upd(lang)
    if kind == "kv":
        return b_kv(blk[1], lang)
    raise ValueError(f"unknown block {kind}")


# ---------------------------------------------------------------- chrome

def page_path(slug):
    return "/" + (slug + "/" if slug else "")


def header(page, lang):
    T = P.T
    menus = []
    for m in P.NAV:
        if "href" in m:
            cur = ' aria-current="page"' if page["slug"] == m["href"].strip("/") else ""
            menus.append(a(m["href"], html.escape(tx(m["label"], lang)), lang, "navlink", cur))
            continue
        cols = []
        for col in m["cols"]:
            links = []
            for it in col["items"]:
                desc = f'<span>{html.escape(tx(it.get("d"), lang))}</span>' if it.get("d") else ""
                links.append(a(it["href"], f'<b>{html.escape(tx(it["l"], lang))}</b>{desc}', lang))
            head = f'<p class="col-h">{html.escape(tx(col["h"], lang))}</p>' if col.get("h") else ""
            cols.append(f'<div class="mcol">{head}{"".join(links)}</div>')
        active = " active" if page.get("group") == m["group"] else ""
        menus.append(f'<details class="dd{active}"><summary>{html.escape(tx(m["label"], lang))}</summary>'
                     f'<div class="panel">{"".join(cols)}</div></details>')
    langs = []
    for l2 in LANGS:
        cur = ' aria-current="true"' if l2 == lang else ""
        langs.append(f'<a hreflang="{l2}" lang="{l2}" href="{PREFIX[l2]}{page_path(page["slug"])}"{cur}><b>{P.LANG_NAME[l2]}</b></a>')
    lang_dd = (f'<details class="dd lang-dd"><summary aria-label="{html.escape(tx(T["lang_label"], lang))}">'
               f'<span class="globe" aria-hidden="true"></span>{P.LANG_SHORT[lang]}</summary>'
               f'<div class="panel">{"".join(langs)}</div></details>')
    return (
        '<header class="top" id="top"><div class="bar">'
        f'<a class="brand" href="{PREFIX[lang] or ""}/"><img src="/img/logo.png" alt="KM Code Craft" width="130" height="55">'
        f'<span>Ultar OS</span></a>'
        f'<button class="menu-btn" type="button" aria-expanded="false" aria-controls="nav">'
        f'<span class="burger" aria-hidden="true"></span>{html.escape(tx(T["menu"], lang))}</button>'
        f'<nav id="nav" class="nav" aria-label="{html.escape(tx(T["nav_label"], lang))}">'
        f'<div class="menus">{"".join(menus)}</div>'
        f'<div class="nav-end">{lang_dd}'
        f'{a("/download/", html.escape(tx(T["get"], lang)), lang, "btn primary small")}</div>'
        '</nav></div></header>'
    )


def subnav(page, lang):
    group = page.get("group")
    tabs = P.SUBNAV.get(group)
    if not tabs or not page.get("slug"):
        return ""
    out = []
    for label, href in tabs:
        cur = ' aria-current="page"' if page["slug"] == href.strip("/") else ""
        out.append(a(href, html.escape(tx(label, lang)), lang, None, cur))
    return f'<nav class="subnav" aria-label="{html.escape(tx(P.T["sub_label"], lang))}"><div class="wrap">{"".join(out)}</div></nav>'


def footer(lang):
    cols = []
    for col in P.FOOTER:
        links = "".join(f"<li>{a(h, html.escape(tx(lbl, lang)), lang)}</li>" for lbl, h in col["items"])
        cols.append(f'<div><h2 class="fh">{html.escape(tx(col["h"], lang))}</h2><ul>{links}</ul></div>')
    T = P.T
    return (
        '<footer class="foot"><div class="wrap">'
        f'<div class="fcols">{"".join(cols)}</div>'
        '<div class="fbottom">'
        f'<p><b>Ultar OS</b> · {html.escape(tx(T["by"], lang))} محمد أنس قرة محمد · Mohamad Anas Kara Mohamad</p>'
        f'<p>© 2026 <a href="https://code-craft.tech" rel="noopener noreferrer">KM Code Craft</a> · '
        f'<a href="mailto:info@code-craft.tech">info@code-craft.tech</a> · '
        f'{a("/privacy/", html.escape(tx(T["privacy"], lang)), lang)} · <a href="#top">{html.escape(tx(T["totop"], lang))}</a></p>'
        '</div></div></footer>'
    )


def page_html(page, lang):
    slug = page["slug"]
    path = page_path(slug)
    title = tx(page["title"], lang)
    full = "Ultar OS – KM Code Craft" if not slug else f"{title} – Ultar OS"
    alts = "".join(f'<link rel="alternate" hreflang="{l2}" href="{SITE}{PREFIX[l2]}{path}">' for l2 in LANGS)
    alts += f'<link rel="alternate" hreflang="x-default" href="{SITE}{path}">'
    body = "".join(render(b, lang) for b in page["blocks"])
    crumbs = ""
    if slug and not page.get("hero"):
        crumbs = f'<section class="phead"><div class="wrap"><h1>{inline(title, lang)}</h1>'
        if page.get("lead"):
            crumbs += f'<p class="lead">{inline(tx(page["lead"], lang), lang)}</p>'
        crumbs += "</div></section>"
    return (
        f'<!doctype html>\n<html lang="{lang}" dir="{"rtl" if lang == "ar" else "ltr"}">\n<head>\n'
        '<meta charset="utf-8">\n<meta name="viewport" content="width=device-width, initial-scale=1">\n'
        '<meta name="referrer" content="no-referrer">\n<meta name="color-scheme" content="dark">\n'
        f'<title>{html.escape(full)}</title>\n'
        f'<meta name="description" content="{html.escape(tx(page.get("desc") or page.get("lead") or P.T["desc"], lang))}">\n'
        f'<link rel="canonical" href="{SITE}{PREFIX[lang]}{path}">{alts}\n'
        '<link rel="icon" href="/img/icon-192.png">\n'
        f'<link rel="stylesheet" href="/style.css?v={VERSION}">\n<script src="/app.js?v={VERSION}" defer></script>\n'
        f'</head>\n<body data-page="{slug or "home"}">\n'
        f'<a class="skip" href="#main">{html.escape(tx(P.T["skip"], lang))}</a>\n'
        f'{header(page, lang)}\n{subnav(page, lang)}\n<main id="main">{crumbs}{body}</main>\n{footer(lang)}\n'
        '</body>\n</html>\n'
    )


def build(out):
    out = Path(out)
    keep_img = out / "img"
    for lang in LANGS:
        for page in P.PAGES:
            d = out / PREFIX[lang].lstrip("/") / page["slug"] if lang != "ar" else out / page["slug"]
            d.mkdir(parents=True, exist_ok=True)
            (d / "index.html").write_text(page_html(page, lang), encoding="utf-8")
    shutil.copyfile(HERE / "style.css", out / "style.css")
    shutil.copyfile(HERE / "app.js", out / "app.js")
    keep_img.mkdir(exist_ok=True)
    print(f"SITE: {len(P.PAGES)} pages x {len(LANGS)} languages -> {out}")


if __name__ == "__main__":
    build(sys.argv[1] if len(sys.argv) > 1 else HERE.parent / "site")

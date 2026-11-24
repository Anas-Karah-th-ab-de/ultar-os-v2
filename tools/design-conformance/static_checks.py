#!/usr/bin/env python3
"""Design conformance – static checks that need no rendering.

Checks (each prints findings; exit 1 on any finding not listed in the known list):
  icons    every `data-icon` used on a board exists in ultar-ui (`src/icon/mod.rs`) with the same
           SVG path data and the same "mirrors in RTL" flag as docs/ui/ICONS.md
  tokens   design/tokens.toml and platform/ultar-ui/tokens.toml are identical; every colour on
           every board is a token colour of the board's theme
  clicks   every `data-act` of every board is in docs/ui/CLICKS.md and the counts agree
  i18n     every key of ultar-ui's English catalogues exists in the Arabic one (no English left
           in Arabic, no placeholder keys), and the reverse
  coverage design/conformance/coverage.toml lists every action of CLICKS.md with a status

Usage: static_checks.py <repo-root> [--inventory out/inventory.json] [--known file]
"""
import argparse
import html
import json
import pathlib
import re
import sys
import tomllib


def board_icons(repo):
    """name -> set of path data strings used for that icon on the boards."""
    out = {}
    pat = re.compile(r'<svg[^>]*data-icon="([a-z0-9-]+)"[^>]*>(.*?)</svg>', re.S)
    for f in sorted((repo / "design/boards/project").glob("*.dc.html")):
        for name, body in pat.findall(f.read_text()):
            d = " ".join(re.findall(r'\bd="([^"]+)"', body))
            others = re.findall(r"<(circle|rect|line|polyline|ellipse)\b", body)
            out.setdefault(name, set()).add(d + ("|" + ",".join(others) if others else ""))
    return out


def toolkit_icons(repo):
    """name -> (path data, mirrors) from the icons! table of ultar-ui."""
    src = (repo / "platform/ultar-ui/src/icon/mod.rs").read_text()
    pat = re.compile(r'\n\s+([A-Z][A-Za-z]*), "([a-z0-9-]+)", (true|false), "(?:[^"\\]|\\.)*",\s*((?:"[^"]*"\s*)+);')
    out = {}
    for _ident, name, mirrors, paths in pat.findall(src):
        d = "".join(re.findall(r'"([^"]*)"', paths))
        out[name] = (d, mirrors == "true")
    return out


def norm(d):
    return re.sub(r"\s+", " ", d.split("|")[0]).strip()


def check_icons(repo, findings):
    b = board_icons(repo)
    t = toolkit_icons(repo)
    icons_md = (repo / "docs/ui/ICONS.md").read_text()
    for name in sorted(b):
        if name not in t:
            findings.append(f"icons: board icon `{name}` is missing in ultar-ui")
            continue
        variants = {norm(v) for v in b[name]}
        if len(variants) > 1:
            findings.append(f"icons: board icon `{name}` is drawn {len(variants)} different ways on the boards")
        if norm(t[name][0]) not in variants and not any(norm(t[name][0]).replace(" ", "") == v.replace(" ", "") for v in variants):
            findings.append(f"icons: `{name}` path differs between board and ultar-ui")
    for name in sorted(set(re.findall(r"^\|\s*`([a-z0-9-]+)`", icons_md, re.M))):
        if name not in t:
            findings.append(f"icons: ICONS.md icon `{name}` is missing in ultar-ui")
    return len(b), len(t)


def hexcolor(c):
    m = re.match(r"rgba?\((\d+), (\d+), (\d+)(?:, ([\d.]+))?\)", c)
    if not m:
        return None, 0.0
    r, g, bl = map(int, m.groups()[:3])
    return "#%02X%02X%02X" % (r, g, bl), float(m.group(4)) if m.group(4) else 1.0


def check_tokens(repo, inv, findings):
    a = (repo / "design/tokens.toml").read_bytes()
    b = (repo / "platform/ultar-ui/tokens.toml").read_bytes()
    if a != b:
        findings.append("tokens: design/tokens.toml and platform/ultar-ui/tokens.toml differ")
    t = tomllib.loads(a.decode())
    allowed = {v.upper() for th in t["theme"].values() for v in th.values()}
    for board, d in (inv or {}).items():
        bad = set()
        for c in d["colors"]:
            h, al = hexcolor(c)
            if h and al > 0 and h not in allowed:
                bad.add(h)
        for c in sorted(bad):
            findings.append(f"tokens: board {board} uses colour {c} that is not a token")


def clicks_table(repo):
    """board -> list of (n, name, action) from CLICKS.md."""
    out, board = {}, None
    for line in (repo / "docs/ui/CLICKS.md").read_text().splitlines():
        m = re.match(r"^## (\w+) \((\d+)\)", line)
        if m:
            board = m.group(1)
            out[board] = []
            continue
        if line.startswith("## "):
            board = None
        m = re.match(r"^\| (\d+) \| (.*?) \| `([a-z.]+)` \|", line)
        if board and m:
            out[board].append((int(m.group(1)), html.unescape(m.group(2)), m.group(3)))
    return out


def check_clicks(repo, inv, findings):
    table = clicks_table(repo)
    for board, d in (inv or {}).items():
        rows = table.get(board)
        if rows is None:
            findings.append(f"clicks: board {board} has no section in CLICKS.md")
            continue
        acts = [a["act"] for a in d["acts"]]
        if len(acts) != len(rows):
            findings.append(f"clicks: {board}: {len(acts)} clickable elements on the board, {len(rows)} in CLICKS.md")
        from collections import Counter
        cb, cc = Counter(acts), Counter(act for _n, _name, act in rows)
        for act in sorted(set(cb) | set(cc)):
            if cb[act] != cc[act]:
                findings.append(f"clicks: {board}: `{act}` {cb[act]}x on the board, {cc[act]}x in CLICKS.md")
        board_names = [(a["act"], a["name"]) for a in d["acts"]]
        for _n, name, act in rows:
            if not any(ba == act and (bn == name or bn.startswith(name) or name in bn) for ba, bn in board_names):
                findings.append(f"clicks: {board}: CLICKS.md names `{act}` {name!r}, the board has no element with that name")
        hidden = [a for a in d["acts"] if a["act"] != "settings.area" and ( a["box"]["x"] + a["box"]["w"] <= 0 or a["box"]["y"] >= d["height"] or a["box"]["x"] >= d["width"])]
        if hidden:
            findings.append(f"clicks: {board}: {len(hidden)} clickable elements lie outside the artboard (not visible): "
                            + ", ".join(sorted({f'#{a["i"]} {a["act"]}' for a in hidden})[:8]))
    return table


def ftl_keys(path):
    keys = {}
    cur = None
    for line in path.read_text().splitlines():
        m = re.match(r"^([a-z][a-z0-9.-]*)\s*=\s*(.*)$", line)
        if m:
            cur = m.group(1)
            keys[cur] = m.group(2)
        elif cur and line.startswith((" ", "\t")):
            keys[cur] += " " + line.strip()
    return keys


def check_i18n(repo, findings):
    dirs = [p for p in repo.glob("**/locales") if "target" not in p.parts and (p / "ar.ftl").exists() and (p / "en.ftl").exists()]
    for d in sorted(dirs):
        ar, en = ftl_keys(d / "ar.ftl"), ftl_keys(d / "en.ftl")
        rel = d.relative_to(repo)
        for k in sorted(set(en) - set(ar)):
            findings.append(f"i18n: {rel}: key `{k}` has no Arabic text")
        for k in sorted(set(ar) - set(en)):
            findings.append(f"i18n: {rel}: key `{k}` has no English text")
        for k, v in ar.items():
            v2 = re.sub(r"\{[^}]*\}", "", v)
            words = re.findall(r"[A-Za-z]{4,}", v2)
            latin_ok = {"Ultar", "Store", "Firefox", "Linux", "Wayland", "Hz", "HDR", "USB", "VPN", "Office", "Mesh", "Ctrl", "Shift", "Super", "Enter", "Space", "Bluetooth"}
            bad = [w for w in words if w not in latin_ok and not w.isupper()]
            if bad and not re.search(r"[؀-ۿ]", v2):
                findings.append(f"i18n: {rel}: Arabic `{k}` is not Arabic: {v[:50]!r}")
            if re.search(r"\b(TODO|FIXME|XXX)\b", v) or v.strip() == k:
                findings.append(f"i18n: {rel}: Arabic `{k}` is a placeholder")


def check_coverage(repo, table, findings):
    path = repo / "design/conformance/coverage.toml"
    if not path.exists():
        findings.append("coverage: design/conformance/coverage.toml is missing")
        return
    cov = tomllib.loads(path.read_text())
    listed = set(cov.get("action", {}))
    used = {a for rows in table.values() for _n, _name, a in rows}
    for a in sorted(used - listed):
        findings.append(f"coverage: action `{a}` of CLICKS.md has no entry in coverage.toml")
    for a in sorted(listed - used):
        findings.append(f"coverage: coverage.toml lists `{a}` that no board uses")
    for a, e in cov.get("action", {}).items():
        if e.get("status") not in ("ok", "different", "missing", "dead"):
            findings.append(f"coverage: `{a}` has an invalid status {e.get('status')!r}")
        if e.get("status") in ("ok", "different") and not e.get("test"):
            findings.append(f"coverage: `{a}` claims `{e.get('status')}` without naming the test that proves it")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("repo")
    ap.add_argument("--inventory")
    ap.add_argument("--known", help="file with accepted findings (one per line); new findings fail")
    ap.add_argument("--only", nargs="*", default=["icons", "tokens", "clicks", "i18n", "coverage"])
    a = ap.parse_args()
    repo = pathlib.Path(a.repo).resolve()
    inv = json.loads(pathlib.Path(a.inventory).read_text()) if a.inventory else None
    findings = []
    table = clicks_table(repo)
    if "icons" in a.only:
        nb, nt = check_icons(repo, findings)
        print(f"icons: {nb} on boards, {nt} in ultar-ui")
    if "tokens" in a.only:
        check_tokens(repo, inv, findings)
    if "clicks" in a.only:
        table = check_clicks(repo, inv, findings)
    if "i18n" in a.only:
        check_i18n(repo, findings)
    if "coverage" in a.only:
        check_coverage(repo, table, findings)
    known = set()
    if a.known and pathlib.Path(a.known).exists():
        known = {l.strip() for l in pathlib.Path(a.known).read_text().splitlines() if l.strip() and not l.startswith("#")}
    new = [f for f in findings if f not in known]
    stale = sorted(known - set(findings))
    for f in findings:
        print(("KNOWN " if f in known else "NEW   ") + f)
    for f in stale:
        print("FIXED (remove from known list) " + f)
    print(f"findings={len(findings)} new={len(new)} fixed={len(stale)}")
    return 1 if new or stale else 0


if __name__ == "__main__":
    sys.exit(main())

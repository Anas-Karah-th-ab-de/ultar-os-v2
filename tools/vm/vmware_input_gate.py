"""The steps of `vmware-input.py gate` (only emulated devices through QMP, no test hook).

Screen positions are those of the desktop bar in Arabic (right to left) on the firmware's 1280x800
frame buffer: the bar fills the bottom 56 points; from the right: start (brand mark), all apps,
search, then the pinned apps files, browser, terminal, editor, store, settings (platform/ultar-desktop
model/apps.rs). Every click is checked on the screen, never assumed."""
import os
import re
import time

# the bar (bottom 56 points, Arabic: right to left): start (the brand mark), 52 points further the app grid,
# then the search field (200 wide), then the pinned apps that are installed, 52 points apart, in the order
# files, browser, terminal, editor, store, settings (the image has no terminal and no editor)
BAR_Y_FROM_BOTTOM = 28
STEP = 52
PINNED_ORDER = ["files", "browser", "store", "settings"]
# the lock screen's sign-in button (centred, 132 points below the middle) and the phone's tile in the
# app grid (not pinned: the grid lists it under "all apps"; 48 left of the middle, 345 from the top),
# both measured at 1280x800 and kept relative to the screen's size
def sign_in_button(w, h):
    return (w // 2, h // 2 + 132)


def phone_tile(w, h):
    return (w // 2 - 48, 345)
def work(vm):
    """The work area above the bar (where windows and menus open)."""
    w, h = vm.size
    return (0, 0, w, h - 60)
APPS = ["settings", "files", "store", "phone", "browser"]


def bar_positions(screen):
    """Start, app grid and the pinned apps, found from the brand mark on the bar (never assumed)."""
    xs = []
    w, h, px = screen.w, screen.h, screen.px
    for y in range(h - 56, h):
        row = y * w * 3
        for x in range(w // 2, w):
            i = row + x * 3
            if abs(px[i] - 0xE3) <= 40 and abs(px[i + 1] - 0xC5) <= 40 and abs(px[i + 2] - 0x5A) <= 40:
                xs.append(x)
    if not xs:
        return None
    y = h - BAR_Y_FROM_BOTTOM
    mark = sum(xs) // len(xs)
    first = mark - STEP - 26 - 200 - 5 - 26  # right edge of the search field, then half an app button
    pos = {"start": (mark, y), "all": (mark - STEP, y)}
    for i, name in enumerate(PINNED_ORDER):
        pos[name] = (first - STEP * i, y)
    return pos


def bright(screen, box):
    """Pixels of text and icons (light foreground) inside box (x0, y0, x1, y1)."""
    x0, y0, x1, y1 = box
    n = 0
    for y in range(max(0, y0), min(screen.h, y1)):
        row = y * screen.w * 3
        for x in range(max(0, x0), min(screen.w, x1)):
            i = row + x * 3
            if screen.px[i] + screen.px[i + 1] + screen.px[i + 2] > 540:
                n += 1
    return n


def black(screen):
    """Share of pure black pixels (where no surface covers the frame buffer), every 4th pixel."""
    n = d = 0
    for y in range(0, screen.h, 4):
        row = y * screen.w * 3
        for x in range(0, screen.w, 4):
            i = row + x * 3
            n += 1
            d += screen.px[i:i + 3] == b"\0\0\0"
    return d / max(1, n)


def lock_screen_checks(vm, s):
    """The sign-in as the Lock board has it: it fills the whole frame buffer (no black band), the
    brand, the avatar, the name and the password field in the middle (Latin and Arabic text drawn),
    the accessibility menu only on a click and then with the names of its four switches."""
    problems = []
    if black(s) > 0.005:
        problems.append(f"the lock screen does not fill the screen ({black(s):.1%} black)")
    middle = bright(s, (s.w // 2 - 170, s.h // 2 - 250, s.w // 2 + 170, s.h // 2 + 70))
    if middle < 400:
        problems.append(f"no brand, avatar, name and field in the middle ({middle} text pixels)")
    panel = (24, s.h - 234, 280, s.h - 56)
    if bright(s, panel) > 100:
        problems.append("the accessibility menu is open without a click")
    vm.click(80, s.h - 40)
    a = None
    end = time.time() + 15
    while time.time() < end:
        vm.pump(1)
        a = vm.shot("access")
        if bright(a, (120, s.h - 234, 190, s.h - 56)) > 300:  # the four switches are there
            break
    words = bright(a, (190, s.h - 234, 280, s.h - 56))
    log(f"lock screen: black {black(s):.3f}, middle {middle}, menu words {words}")
    if words < 150:
        problems.append(f"the accessibility switches have no names ({words} text pixels)")
    vm.click(80, s.h - 40)  # closes it again
    vm.pump(1)
    return problems


def knobs(screen, h):
    """Centres of the enabled switches' knobs in the open accessibility menu (bright round knobs in
    the menu's start column), top to bottom."""
    rows = []
    for y in range(h - 300, h - 56):
        xs = [x for x in range(30, 110) if sum(screen.px[(y * screen.w + x) * 3:(y * screen.w + x) * 3 + 3]) > 690]
        if len(xs) >= 6:
            rows.append((y, sum(xs) // len(xs)))
    out = []
    for y, x in rows:
        if out and y - out[-1][1][-1] <= 2:
            out[-1][1].append(y)
            out[-1][0].append(x)
        else:
            out.append(([x], [y]))
    return [(xs[len(xs) // 2], ys[len(ys) // 2]) for xs, ys in out if len(ys) >= 6]


def clipboard(vm, m, pos):
    """Ctrl+A, Ctrl+C in Settings' search field, Ctrl+V in Files' search field: the text arrives in
    the other app (compositor clipboard, keys with Ctrl through the emulated keyboard)."""
    w, h = vm.size
    cx, cy = w // 2, h // 2
    vm.move(cx, cy)
    vm.pump(1)
    empty = vm.shot("clip-empty")
    vm.click(*pos["settings"])
    if not app_window(vm, "clip-settings", empty):
        return ["clipboard: Settings did not open"]
    vm.click(cx + 240, cy - 192)  # Settings' search field (window in the middle, field at its top end)
    vm.pump(1)
    vm.type("ultar")
    vm.pump(1)
    vm.key("ctrl+a")
    vm.key("ctrl+c")
    vm.pump(1)
    typed = vm.shot("clip-typed")
    # Settings stays open (it serves the copied text), Files opens above it
    vm.click(*pos["files"])
    vm.pump(6)  # its window is above Settings (the paste below shows it is there)
    # a second window opens 40 points down and to the side of the first (cascade): find which
    shot = vm.shot("clip-files")
    ground = bytes.fromhex("0B0D11")
    off = 40 if all(shot.px[(y * w + cx + 400) * 3:(y * w + cx + 400) * 3 + 3] != ground
                    for y in range(cy - 100, cy + 100, 20)) else 0
    field = (cx - 300 + off, cy - 216 + off, cx - 90 + off, cy - 176 + off)  # Files' search field
    vm.click(cx - 193 + off, cy - 196 + off)
    vm.pump(1)
    before = vm.shot("clip-before-paste")
    vm.key("ctrl+v")
    ok, after = m.wait_for(vm, "clip-pasted", lambda x: x.diff(before, field) > 0.03, 15)
    close_all(vm)
    log(f"clipboard: Files' search field changed {after.diff(before, field):.3f} after Ctrl+V")
    close_all(vm)
    return [] if ok else ["clipboard: Ctrl+V in Files did not paste what Ctrl+C copied in Settings"]


def pure_black(screen):
    """Share of pixels of the high-contrast ground (#000000), every 4th pixel."""
    return black(screen)


def accessibility(vm, m):
    """Lock again (Super+L), open the sign-in's accessibility menu and switch on high contrast and
    large text: the lock screen must change at once (black ground, larger text), and after a restart
    of the machine it must still show high contrast (the settings service stored it)."""
    problems = []
    vm.key("meta_l+l")
    ok, s = m.wait_for(vm, "relocked", lambda x: m.locked(x.facts()), 30)
    if not ok:
        return ["Super+L did not lock"]
    sign_in(vm)
    w, h = vm.size
    vm.click(80, h - 40)
    vm.pump(2)
    before = vm.shot("a11y-before")
    middle = (w // 2 - 170, h // 2 - 250, w // 2 + 170, h // 2 + 70)
    text_before = bright(before, middle)
    switches = knobs(before, h)
    log(f"accessibility: switches at {switches}")
    if len(switches) < 2:
        return problems + [f"the accessibility menu has no switches ({switches})"]
    vm.click(*switches[1])  # high contrast (second switch)
    ok, hc = m.wait_for(vm, "a11y-contrast", lambda x: pure_black(x) > 0.6, 20)
    log(f"accessibility: high contrast black ground {pure_black(hc):.2f} (before {pure_black(before):.2f})")
    if not ok:
        problems.append("high contrast changed nothing on the lock screen")
    text_hc = bright(hc, middle)
    vm.click(*switches[0])  # large text (first switch)
    ok, lt = m.wait_for(vm, "a11y-large", lambda x: bright(x, middle) > 1.05 * text_hc and x.diff(hc, middle) > 0.02, 20)
    log(f"accessibility: text pixels {text_before} -> {text_hc} (contrast) -> {bright(lt, middle)} (large text)")
    if not ok:
        problems.append("large text changed nothing on the lock screen")
    # survives a restart: the machine is reset, the lock screen starts with high contrast
    vm.pump(3)
    mark = len(vm.buf)
    vm.command("system_reset")
    end = time.time() + 300
    while time.time() < end and b"] settled" not in vm.buf[mark:]:
        vm.pump(1)
    ok, after = m.wait_for(vm, "a11y-restart", lambda x: pure_black(x) > 0.6 and x.facts()["mark"] >= 20, 120)
    log(f"accessibility: after the restart black ground {pure_black(after):.2f}")
    if not ok:
        problems.append("high contrast did not survive a restart")
    return problems


def log(msg):
    print(msg, flush=True)


def _screen_text():
    import importlib.util
    import os
    spec = importlib.util.spec_from_file_location("screen_text", os.path.join(os.path.dirname(__file__), "screen_text.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# ---------------------------------------------------------------------------------------------------------
# App content (branch build/appbus): the apps reach the system services from their cells. Settings shows real
# values on every page, a change takes effect and survives a restart; the Store and Files list their content.
# Everything below up to `installed` belongs to this block.
#
# The sidebar of Settings in its order (apps/ultar-settings/src/areas.rs `wide()`: every area but the mobile
# network). The gate moves through it with the keyboard (Tab, arrows, Enter - the test
# `the_keyboard_alone_opens_a_page_and_changes_its_first_setting` of the app holds the same keys) and reads
# each page from the screen.
SETTINGS_AREAS = [
    "wifi", "network", "bluetooth", "displays", "sound", "power", "desktop", "windows", "appearance",
    "notifications", "search", "apps", "mouse", "keyboard", "printers", "colour", "accessibility", "privacy",
    "sharing", "accounts", "users", "region", "datetime", "storage", "drivers", "updates", "backup", "mesh",
    "assistant", "cells", "emergency", "about",
]
# The pages a service stands behind, with words that tell the page is the one asked for (of its title, ar.ftl
# app.settings.area.* and app.settings.account.title, or of its first rows: the recogniser does not read every
# large title) and words the page shows only with the service's real values (a row label or a value;
# normalised, see screen_text.norm). None of the "not available" messages may appear on
# them (ar.ftl app.settings.error.unavailable, .status.unavailable, .panel.unavailable, .no_service,
# .display.no_outputs/no_settings, .account.unavailable) and no catalog key.
SETTINGS_PAGES = [
    ("network", ["الشبكة", "السلكي"], ["السلكي", "الطيران"]),
    ("bluetooth", ["بلوتوث", "البلوتوث"], ["المتصلة", "البلوتوث"]),
    ("displays", ["الشاشات", "ترتيب"], ["ترتيب", "اسحب"]),
    ("sound", ["الصوت", "المخرج"], ["مستوى", "المخرج"]),
    ("power", ["الطاقة"], []),
    ("desktop", ["المكتب", "والمرسى", "الشريط"], []),
    ("windows", ["النوافذ"], []),
    ("appearance", ["المظهر", "السمة"], []),
    ("notifications", ["الإشعارات", "الإزعاج"], []),
    ("privacy", ["الخصوصية", "والأمان", "الميكروفون", "الكاميرا"], []),
    ("accounts", ["حساب"], []),
    ("region", ["المنطقة", "واللغة", "لغة"], []),
    ("datetime", ["التاريخ", "والوقت", "تلقائيا"], []),
    ("drivers", ["العتاد", "والتعريفات", "تعريف"], []),
    ("about", ["حول", "0.8"], ["0.8"]),
]
# a catalog key on the screen (settings.power.button_action, accountd.link.no_server)
RAW_KEY = re.compile(r"\b(?:app|settings|accountd|devd|update|brand|desk)\.[a-z0-9_]{2,}(?:\.[a-z0-9_]+)+\b")
# the distinctive words of the "not available" messages (the account page's own "there is no account server
# yet" is the honest state of a device without account server, not a broken page)
UNAVAILABLE = ["تعذرت", "تعذر", "لا تعمل", "لا يسمح", "غير متاح في هذا", "غير متاحة"]
# width of Settings' sidebar (apps/ultar-settings/src/view/metrics.rs) and height of a window's title bar
SIDEBAR = 288
TITLEBAR = 40


def _page_text(st, png, right, bottom):
    """The words on the screen left of `right` and above `bottom` (the whole screen is read: the recogniser
    does better with the full picture than with a cut-out)."""
    return st.norm(" ".join(x for x, b, _ in st.words(png) if b[0] < right and b[1] < bottom))


def _changed_box(a, b, box):
    """The bounding box of what differs between two screens inside box (every 4th pixel), or None."""
    x0, y0, x1, y1 = box
    xs, ys = [], []
    for y in range(y0, y1, 4):
        row = y * a.w * 3
        for x in range(x0, x1, 4):
            i = row + x * 3
            if a.px[i:i + 3] != b.px[i:i + 3]:
                xs.append(x)
                ys.append(y)
    if not xs:
        return None
    return (min(xs), min(ys), max(xs) + 4, max(ys) + 4)


def _open_settings(vm, pos, tag):
    """Clicks Settings on the bar; the window's box on the screen once it is there, or None. The pointer is
    left on the bar (no hover in the window), no widget of the window has the focus yet."""
    vm.move(vm.size[0] // 2, vm.size[1] // 2)
    vm.pump(1)
    before = vm.shot(f"before-{tag}")
    vm.click(*pos["settings"])
    if not app_window(vm, tag, before):
        return None
    vm.move(pos["settings"][0], pos["settings"][1])
    vm.pump(3)
    shot = vm.shot(f"{tag}-window")
    box = _changed_box(before, shot, work(vm))
    if not box:
        return None
    # what changed includes the window's shadow: the window itself starts where its title bar does (a
    # colour far from the screen before), found from above and from both sides
    def far(x, y):
        i = (y * shot.w + x) * 3
        return sum(abs(int(p) - int(q)) for p, q in zip(shot.px[i:i + 3], before.px[i:i + 3])) > 24

    x0, y0, x1, y1 = box
    mid = (x0 + x1) // 2
    top = next((y for y in range(y0, y1) if far(mid, y)), y0)
    left = next((x for x in range(x0, mid) if far(x, top + 12)), x0)
    right = next((x for x in range(x1 - 1, mid, -1) if far(x, top + 12)), x1 - 1) + 1
    return (left, top, right, y1 - (left - x0))


def _keys(vm, key, n):
    for _ in range(n):
        vm.key(key)


def _goto(vm, area, focused):
    """Opens the page `area` with the keyboard: from a window without focus Tab three times reaches the first
    sidebar entry (the sidebar, the search field, the entry); the arrows move to the entry, Enter opens it
    and the focus stays on it. Returns the entry that has the focus now."""
    want = SETTINGS_AREAS.index(area)
    if focused is None:
        _keys(vm, "tab", 3)
        focused = 0
    _keys(vm, "down" if want > focused else "up", abs(want - focused))
    vm.key("ret")
    vm.pump(2.5)
    return want


def _first_control(vm, focused, box, tag):
    """Moves the focus from the sidebar entry `focused` to the first control of the open page: Tab past the
    last entry of the sidebar. A page longer than the window takes the focus itself first (it scrolls with
    the keys; its focus ring runs along the window's edge): then one Tab more."""
    _keys(vm, "tab", len(SETTINGS_AREAS) - focused)
    vm.pump(0.8)
    s = vm.shot(f"{tag}-focus")
    y = (box[1] + box[3]) // 2
    ring = False
    for x in range(box[0], box[0] + 5):
        r, g, b = s.px[(y * s.w + x) * 3:(y * s.w + x) * 3 + 3]
        ring = ring or (b > 150 and b > r + 40)
    if ring:
        vm.key("tab")
        vm.pump(0.5)
    return ring


def _read_page(st, vm, name, box):
    """The text of the page part of the Settings window `box` (everything but the title bar and the sidebar,
    which is at the right in Arabic): the whole screen read, and the page cut out and enlarged read again
    (small text) - both, so that a word one reading misses is still found."""
    vm.shot(name)
    png = f"{vm.prefix}-{name}.png"
    x0, y0, x1, y1 = box
    page = (x0, y0 + TITLEBAR, max(x0 + 100, x1 - SIDEBAR), y1)
    whole = " ".join(x for x, b, _ in st.words(png) if page[0] <= b[0] < page[2] and page[1] <= b[1] < page[3])
    title = (page[0], page[1], page[2], min(page[3], page[1] + 100))
    cut = " ".join(x for x, _, _ in st.words(png, page) + st.words(png, title, psm=6))
    return st.norm(whole + " " + cut)


def _on_page(st, text, title):
    return any(st.norm(word) in text for word in title)


def settings_pages(vm, m, pos):
    """Opens Settings and each page of SETTINGS_PAGES with the keyboard: the page is the one asked for (its
    title), shows real values and no "not available" message or catalog key."""
    st = _screen_text()
    failures = []
    box = _open_settings(vm, pos, "settings-pages")
    if not box:
        return ["settings did not open (pages)"]
    log(f"settings window: {box}")
    focused = None
    for area, title, expect in SETTINGS_PAGES:
        text = ""
        for attempt in (1, 2, 3):
            focused = _goto(vm, area, focused)
            text = _read_page(st, vm, f"settings-page-{area}", box)
            if _on_page(st, text, title):
                break
            # a key was lost (or the page is still being drawn): once more from a new window
            log(f"settings page {area}: title {title} not on the page (try {attempt}), read: {text[:120]!r}")
            close_all(vm)
            box = _open_settings(vm, pos, f"settings-pages-{area}-{attempt}")
            focused = None
            if not box:
                return failures + [f"settings did not open again ({area})"]
        if not _on_page(st, text, title):
            failures.append(f"settings {area}: the page did not open")
            continue
        bad = [b for b in UNAVAILABLE if st.norm(b) in text]
        bad += [k.group(0) for k in RAW_KEY.finditer(text)]
        good = not expect or any(st.norm(e) in text for e in expect)
        log(f"settings page {area}: unavailable={bad} values={'yes' if good else 'no'} ({len(text)} chars)")
        if bad:
            failures.append(f"settings {area}: 'not available' or a raw key on the page {bad}")
        if not good:
            failures.append(f"settings {area}: no real values ({expect})")
    close_all(vm)
    return failures


def _ground(screen, box):
    """Share of the desktop's ground colour (no window there) inside box, every 8th pixel."""
    x0, y0, x1, y1 = box
    n = g = 0
    for y in range(y0, y1, 8):
        row = y * screen.w * 3
        for x in range(x0, x1, 8):
            n += 1
            g += screen.px[row + x * 3:row + x * 3 + 3] == bytes.fromhex("0B0D11")
    return g / max(1, n)


def settings_maximized(vm, m, pos):
    """The maximize button of Settings' window (pointer): the window fills the work area and the Displays page
    is laid out again for the new width - the card's title and texts are complete in the normal window and
    in the maximized one, read from the screen (that nothing is cut off or squeezed at any width is the
    app's test tests/layout.rs). A second click puts the window back."""
    st = _screen_text()
    w, h = vm.size
    box = _open_settings(vm, pos, "settings-max")
    if not box:
        return ["settings did not open (maximize)"]
    _goto(vm, "displays", None)
    normal = _read_page(st, vm, "settings-max-normal", box)
    # the title bar's buttons at its left end (Arabic): close, maximize, minimize, 36 points apart
    vm.click(box[0] + 56, box[1] + 20)
    vm.pump(4)
    area = (0, 0, w, h - 60)
    wide = _read_page(st, vm, "settings-max-wide", area)
    big = vm.shot("settings-max-big")
    failures = []
    # maximized: the window's title bar runs along the top of the screen (the desktop's ground is gone there)
    top = (0, 4, w, TITLEBAR - 4)
    left = _ground(big, top)
    log(f"settings maximized: window was {box}, desktop ground left along the top {left:.2f}")
    if left > 0.1:
        failures.append("settings: the maximize button did not maximize the window")
    want = ["ترتيب", "اسحب"]
    for name, text in (("normal window", normal), ("maximized", wide)):
        missing = [x for x in want if st.norm(x) not in text]
        log(f"settings displays, {name}: missing {missing} ({len(text)} chars)")
        # one of the two may escape the recogniser in a small window; none of them: the page is not there
        if len(missing) == len(want) or (name == "maximized" and missing):
            failures.append(f"settings displays, {name}: text cut off or missing {missing}")
    vm.click(56, 20)  # the same button of the maximized window: back to its size
    vm.pump(3)
    if _ground(vm.shot("settings-max-back"), top) < 0.9:
        failures.append("settings: the window did not come back from maximized")
    close_all(vm)
    return failures


def store_and_files(vm, m, pos):
    """The Store lists its catalogue (publisher and app names on the screen, no load error); Files lists the
    home folder (its first file README.txt, no "not available")."""
    st = _screen_text()
    failures = []
    w, h = vm.size
    for name, want, wait in (("store", ["مستندات", "وجداول", "Ultar"], 10), ("files", ["README"], 4)):
        vm.move(640, 400)
        vm.pump(1)
        before = vm.shot(f"before-{name}-content")
        vm.click(*pos[name])
        if not app_window(vm, f"{name}-content", before):
            failures.append(f"{name} did not open (content)")
            continue
        vm.pump(wait)
        vm.shot(f"{name}-content-ready")
        page = _page_text(st, f"{vm.prefix}-{name}-content-ready.png", w, h - 60)
        bad = [b for b in UNAVAILABLE if st.norm(b) in page]
        good = any(st.norm(x) in page for x in want)
        log(f"{name} content: unavailable={bad} listed={'yes' if good else 'no'} ({len(page)} chars)")
        if bad or not good:
            failures.append(f"{name}: " + ("'not available' shown" if bad else f"nothing listed ({want})"))
        close_all(vm)
    return failures


def _strip_colour(screen, y0, y1):
    """The most frequent colour of the rows y0..y1 (every 4th pixel) and its share."""
    seen = {}
    n = 0
    for y in range(y0, y1, 4):
        row = y * screen.w * 3
        for x in range(0, screen.w, 4):
            c = screen.px[row + x * 3:row + x * 3 + 3]
            seen[c] = seen.get(c, 0) + 1
            n += 1
    colour = max(seen, key=seen.get)
    return colour, seen[colour] / max(1, n)


def _light_bar(screen):
    """The colour of the desktop bar when it is drawn in the light theme, else None: the bottom 56 points are
    mostly one light colour that is not the colour of the screen above them (a light lock screen has no bar)."""
    bar, share = _strip_colour(screen, screen.h - 52, screen.h - 4)
    above, _ = _strip_colour(screen, screen.h - 200, screen.h - 70)
    return bar if share >= 0.5 and sum(bar) > 540 and bar != above else None


def _choose_theme(vm, pos, tag, step):
    """Settings > Appearance, the first setting (the theme: dark, light, automatic), with the keyboard: Space
    opens the choice, `step` (down: the next one, up: the one before) and Enter pick. False when the page
    was not reached."""
    st = _screen_text()
    box = _open_settings(vm, pos, f"theme-{tag}")
    if not box:
        return False
    focused = _goto(vm, "appearance", None)
    text = _read_page(st, vm, f"theme-{tag}-page", box)
    if not _on_page(st, text, dict((a, t) for a, t, _ in SETTINGS_PAGES)["appearance"]):
        log(f"theme ({tag}): the Appearance page is not open, read: {text[:120]!r}")
        close_all(vm)
        return False
    ring = _first_control(vm, focused, box, f"theme-{tag}")
    log(f"theme ({tag}): Appearance open, the page {'scrolls (one Tab more)' if ring else 'fits'}")
    vm.key("spc")
    vm.pump(1.5)
    vm.shot(f"theme-{tag}-choice")
    vm.key(step)
    vm.pump(0.5)
    vm.key("ret")
    vm.pump(1)
    return True


def settings_change(vm, m, pos):
    """A change in Settings takes effect and stays (settingsd stores it and tells the services): the theme
    is switched to light on the Appearance page - the desktop shell's bar turns light at once; after a
    restart of the machine and the sign-in the bar is light again; then the theme is switched back to dark
    (the bar is dark again), as the image was."""
    failures = []
    for attempt in (1, 2):
        if not _choose_theme(vm, pos, f"light-{attempt}", "down"):
            close_all(vm)
            continue
        ok, s = m.wait_for(vm, "theme-light", lambda x: _light_bar(x) is not None, 20)
        if ok:
            break
        close_all(vm)
        if not m.desktop(vm.shot("theme-not-light").facts()):
            break
    else:
        return ["settings change: the Appearance page or its theme choice was not reached"]
    light = _light_bar(s)
    log(f"settings change: light theme chosen, bar {light.hex() if light else None} (dark: 12151b)")
    if not light:
        close_all(vm)
        return ["settings change: choosing the light theme did not change the desktop"]
    close_all(vm)
    vm.pump(3)
    # survives a restart: the machine is reset; after the sign-in the desktop is light
    mark = len(vm.buf)
    vm.command("system_reset")
    end = time.time() + 300
    while time.time() < end and b"] settled" not in vm.buf[mark:]:
        vm.pump(1)
    vm.pump(6)
    after = None
    for attempt in (1, 2, 3):
        if _light_bar(vm.shot("theme-restart-locked")) is not None:
            failures.append("settings change: the desktop is visible before the sign-in after the restart")
            break
        sign_in(vm)
        vm.type(m.password())
        vm.pump(0.5)
        vm.key("ret")
        vm.pump(1)
        vm.click(*sign_in_button(*vm.size))
        ok, s = m.wait_for(vm, f"theme-restart-{attempt}", lambda x: _light_bar(x) is not None or m.desktop(x.facts()), 40)
        if ok:
            after = _light_bar(s)
            break
        vm.pump(8)
    log(f"settings change: after the restart the bar is {after.hex() if after else 'not light'}")
    if after != light:
        failures.append("settings change: the light theme did not survive a restart")
    # the app registrations of this start, then back to the dark theme
    want = [re.compile(rb"\] exit app-" + a.encode() + rb" pid=\d+ code=0 ") for a in APPS]
    end = time.time() + 240
    while time.time() < end and not all(r.search(vm.buf[mark:]) for r in want):
        vm.pump(1)
    vm.pump(3)
    dark = m.desktop(vm.shot("theme-after-restart").facts())
    for attempt in (1, 2):
        if dark:
            break
        if _choose_theme(vm, pos, f"dark-{attempt}", "up"):
            dark, _ = m.wait_for(vm, "theme-dark", lambda x: m.desktop(x.facts()), 20)
        close_all(vm)
        dark = dark or m.desktop(vm.shot("theme-is-dark").facts())
    if not dark:
        failures.append("settings change: switching back to the dark theme did not change the desktop")
    return failures


# end of the app content block (build/appbus)
# ---------------------------------------------------------------------------------------------------------


def installed(vm, m, seconds):
    """Waits until ultar-init reported every app installation as ended with code 0."""
    want = [re.compile(rb"\] exit app-" + a.encode() + rb" pid=\d+ code=0 ") for a in APPS]
    end = time.time() + seconds
    while time.time() < end:
        if all(r.search(vm.buf) for r in want):
            return True
        if not vm.pump(1):
            break
    missing = [a for a, r in zip(APPS, want) if not r.search(vm.buf)]
    log(f"apps not installed: {missing}")
    return False


def app_window(vm, name, before, seconds=60, unlike=None):
    """True when a window appeared: a large part of the work area differs from the empty desktop (and,
    with `unlike`, from that screen too: the app grid the app was started from must be gone)."""
    end = time.time() + seconds
    while time.time() < end:
        vm.pump(2)
        s = vm.shot(name)
        # windows open at their own size: 15 % of the work area at 1280x800, less on larger screens
        w, h = vm.size
        need = min(0.15, 0.15 * 1280 * 740 / (w * (h - 60)))
        changed = s.diff(before, work(vm))
        if changed >= need and (unlike is None or s.diff(unlike, work(vm)) >= need):
            log(f"{name}: window ({changed:.2f} of the work area changed)")
            return True
    log(f"{name}: no window ({changed:.2f} of the work area changed)")
    return False


def close_all(vm):
    """Closes the front window with Ctrl+W, Alt+F4 as fallback; ends with the empty desktop."""
    for combo in ("alt+f4", "ctrl+w", "alt+f4"):
        vm.key(combo)
        vm.pump(1.5)


def sign_in(vm):
    """From wherever the lock screen is: Escape (back to the cover), a key (the sign-in), an empty field."""
    vm.key("esc")
    vm.pump(1)
    vm.key("spc")
    vm.pump(1.5)
    vm.key("ctrl+a")
    vm.key("backspace")
    vm.pump(0.5)


def run(vm, m):
    failures = []
    err = m.boot(vm)
    if err:
        log(f"VMWARE-INPUT: FEHLER {err}")
        return 1
    # 1. locked from the first frame: the desktop bar must never be on the screen before the unlock
    seen_desktop = False
    ok = False
    end = time.time() + 120
    while time.time() < end:
        s = vm.shot("start")
        f = s.facts()
        if m.desktop(f):
            seen_desktop = True
            break
        if m.locked(f) and f["mark"] >= 20:
            ok = True
            break
        vm.pump(0.5)
    log(f"screen 1 (after boot): {f}")
    if seen_desktop or not ok:
        log("VMWARE-INPUT: FEHLER " + ("the desktop is visible before the unlock" if seen_desktop else f"no lock screen {f}"))
        return 1
    cover = s
    # 2. a key opens the sign-in (USB or PS/2 keyboard), the password unlocks
    vm.key("spc")
    ok, s = m.wait_for(vm, "signin", lambda x: x.diff(cover) > 0.01, 30)
    log(f"screen 2 (sign-in): changed={s.diff(cover):.3f}")
    if not ok:
        log("VMWARE-INPUT: FEHLER no key reached the lock screen")
        return 1
    if not m.locked(s.facts()):
        log("VMWARE-INPUT: FEHLER the sign-in is not locked")
        return 1
    problems = lock_screen_checks(vm, s)
    if problems:
        log("VMWARE-INPUT: FEHLER " + "; ".join(problems))
        return 1
    # a wrong password keeps the session locked
    sign_in(vm)
    vm.type("13579")
    vm.pump(0.5)
    vm.shot("typed-wrong")
    vm.key("ret")
    vm.pump(8)
    s = vm.shot("wrong")
    if not m.locked(s.facts()) or m.desktop(s.facts()):
        log("VMWARE-INPUT: FEHLER a wrong password unlocked")
        return 1
    vm.pump(10)  # the wait after one failure (1 s, doubling)
    # the password unlocks at the first try (no second try: a session that locks itself again right
    # after the unlock - a late lock frame, found by this gate - must fail here)
    for attempt in (1,):
        sign_in(vm)
        vm.type(m.password())
        vm.pump(0.5)
        vm.shot(f"typed-{attempt}")
        # Enter, and the sign-in button with the pointer: a lone key press can be lost by the kernel's
        # keyboard polling on a busy machine (desktop-qemu.py resends keys for the same reason); a
        # second submit while the first is checked changes nothing (the lock screen is "checking")
        vm.key("ret")
        vm.pump(1)
        vm.click(*sign_in_button(*vm.size))
        ok, s = m.wait_for(vm, f"desktop-{attempt}", lambda x: m.desktop(x.facts()), 45)
        log(f"screen 3 (desktop after the unlock, try {attempt}): {s.facts()}")
    if ok:
        vm.pump(5)
        ok = m.desktop(vm.shot("desktop-stays").facts())
    if not ok:
        log("VMWARE-INPUT: FEHLER the password did not unlock (or the session locked again)")
        return 1
    if not installed(vm, m, 240):
        failures.append("apps not installed")
    vm.move(640, 400)
    vm.pump(2)
    empty = vm.shot("empty")
    pos = bar_positions(empty)
    log(f"bar: {pos}")
    if not pos:
        log("VMWARE-INPUT: FEHLER no start button on the bar")
        return 1
    # 3. the start button (pointer: USB tablet or VMware absolute mouse) opens the start menu
    vm.click(*pos["start"])
    ok, s = m.wait_for(vm, "startmenu", lambda x: x.diff(empty, work(vm)) > 0.01, 30)
    log(f"screen 4 (start menu): changed={s.diff(empty, work(vm)):.3f}")
    if not ok:
        log("VMWARE-INPUT: FEHLER the start button opened nothing")
        return 1
    vm.key("esc")
    vm.pump(2)
    # GATE_ONLY=content (for working on the gate, never a verdict): only the app content steps
    if os.environ.get("GATE_ONLY") == "content":
        failures = settings_pages(vm, m, pos) + settings_maximized(vm, m, pos) + store_and_files(vm, m, pos)
        failures += settings_change(vm, m, pos)
        log("VMWARE-INPUT: GATE_ONLY=content " + ("FEHLER " + "; ".join(failures) if failures else "ok (no verdict)"))
        return 1
    # 4. every system app opens its window: Settings, Files, Store, Browser from the bar, Phone from
    #    the app grid
    for name in ("settings", "files", "store", "browser"):
        vm.move(640, 400)
        vm.pump(1)
        before = vm.shot(f"before-{name}")
        vm.click(*pos[name])
        if not app_window(vm, f"app-{name}", before):
            failures.append(f"{name} did not open")
        close_all(vm)
    vm.move(640, 400)
    vm.pump(1)
    before = vm.shot("before-phone")
    vm.click(*pos["all"])
    ok, grid = m.wait_for(vm, "allapps", lambda x: x.diff(before, work(vm)) > 0.1, 30)
    if not ok:
        failures.append("the app grid did not open")
    # the phone is the fifth of "all apps" (sorted by their Arabic names), the grid's second row
    vm.click(*phone_tile(*vm.size))
    if not app_window(vm, "app-phone", before, unlike=grid):
        failures.append("phone did not open")
    # 5. the screen keeps changing by itself: the bar's clock turns within a minute
    close_all(vm)
    clock = (0, vm.size[1] - 56, 140, vm.size[1])
    first = vm.shot("clock-1")
    ok, _ = m.wait_for(vm, "clock-2", lambda x: x.diff(first, clock) > 0.002, 75)
    if not ok:
        failures.append("the clock did not tick (static screen)")
    if not failures:
        failures += settings_pages(vm, m, pos)
        failures += settings_maximized(vm, m, pos)
        failures += store_and_files(vm, m, pos)
        failures += settings_change(vm, m, pos)
    if not failures:
        failures += clipboard(vm, m, pos)
    if not failures:
        failures += accessibility(vm, m)
    if failures:
        log("VMWARE-INPUT: FEHLER " + "; ".join(failures))
        return 1
    log("VMWARE-INPUT: ALLES OK (locked at start, keyboard, pointer, start menu, 5 apps, settings pages, maximized, a change that takes effect and survives a restart, store, files)")
    return 0

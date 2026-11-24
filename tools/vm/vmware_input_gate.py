"""The steps of `vmware-input.py gate` (only emulated devices through QMP, no test hook).

Screen positions are those of the desktop bar in Arabic (right to left) on the firmware's 1280x800
frame buffer: the bar fills the bottom 56 points; from the right: start (brand mark), all apps,
search, then the pinned apps files, browser, terminal, editor, store, settings (platform/ultar-desktop
model/apps.rs). Every click is checked on the screen, never assumed."""
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


# Settings pages the gate opens from the sidebar (Arabic labels, ar.ftl app.settings.area.*), with a word each
# page shows only when it has the service's real values (a row label or a value; normalised, see
# screen_text.norm). On every page none of the "not available" messages may appear: their distinctive words
# (ar.ftl app.settings.error.unavailable, .status.unavailable, .panel.unavailable, .no_service,
# .display.no_outputs/no_settings, .account.unavailable) are in UNAVAILABLE, and no raw catalog key.
SETTINGS_PAGES = [
    # in the sidebar's order (the gate scrolls it down only)
    ("network", "الشبكة", ["السلكي", "الطيران"]),
    ("bluetooth", "بلوتوث", ["المتصلة", "البلوتوث"]),
    ("displays", "الشاشات", ["اسحب", "الليلي"]),
    ("sound", "الصوت", ["مستوى", "المخرج"]),
    # settingsd's pages and the drivers: every text translated (no raw key such as settings.power.x)
    ("power", "الطاقة", []),
    ("desktop", "سطح المكتب", []),
    ("windows", "النوافذ", []),
    ("appearance", "المظهر", []),
    ("notifications", "الإشعارات", []),
    ("privacy", "الخصوصية", []),
    ("accounts", "الحسابات", []),
    ("region", "المنطقة واللغة", []),
    ("datetime", "التاريخ والوقت", []),
    ("drivers", "العتاد", []),
    ("about", "حول النظام", ["0.8"]),
]
# a catalog key on the screen: lower-case words joined by dots (settings.power.button_action, accountd.link.x)
RAW_KEY = re.compile(r"\b[a-z]{3,}\.[a-z_]{2,}(\.[a-z_]+)*\b")
# the distinctive words of those messages (the account page's own "the account server is not available yet" is
# the honest state of a device without account server, not a broken page)
UNAVAILABLE = ["تعذرت", "تعذر", "لا تعمل", "لا يسمح", "غير متاح في هذا", "غير متاحة"]


def _sidebar_hit(st, png, label, w, h):
    """Centre of the sidebar entry `label`: the rightmost place the words appear (the sidebar is on the right
    in Arabic; the page itself may use the same word)."""
    want = [st.norm(x) for x in label.split()]
    best = None
    # the whole screen first (read best), then the right half enlarged
    for ws in (
        [x for x in st.words(png) if x[1][0] > w // 2 and x[1][1] < h - 60],
        st.words(png, (w // 2, 0, w, h - 60)),
    ):
        for i, (word, b, _) in enumerate(ws):
            if all(i + k < len(ws) and want[k] in st.norm(ws[i + k][0]) for k in range(len(want))):
                if best is None or b[0] > best[0]:
                    best = ((min(b[0], ws[i + len(want) - 1][1][0]) + max(b[2], ws[i + len(want) - 1][1][2])) // 2, (b[1] + b[3]) // 2)
        if best:
            return best
    return best


def _page_text(st, png, right, bottom):
    """The words on the screen left of `right` and above `bottom` (the whole screen is read: the recogniser
    does better with the full picture than with a cut-out)."""
    return st.norm(" ".join(x for x, b, _ in st.words(png) if b[0] < right and b[1] < bottom))


def settings_pages(vm, m, pos):
    """Opens Settings, then each page of SETTINGS_PAGES from its sidebar entry (found on the screen): the page
    shows real values and no "not available" message."""
    st = _screen_text()
    failures = []
    vm.move(640, 400)
    vm.pump(1)
    before = vm.shot("before-settings-pages")
    vm.click(*pos["settings"])
    if not app_window(vm, "settings-pages", before):
        return ["settings did not open (pages)"]
    vm.pump(4)
    w, h = vm.size
    x_side = None
    for area, label, expect in SETTINGS_PAGES:
        hit = None
        # not on the screen: scroll the sidebar (wheel over it) to its top, then down step by step
        for step, wheel in enumerate([None] + ["wheel-down"] * 9):
            if wheel:
                vm.move(x_side or (w * 3) // 4, h // 2)
                for _ in range(3 if step else 1):
                    vm.events([{"type": "btn", "data": {"down": True, "button": wheel}}])
                    vm.events([{"type": "btn", "data": {"down": False, "button": wheel}}])
                    vm.pump(0.2)
                vm.pump(1)
            vm.shot(f"settings-find-{area}")
            hit = _sidebar_hit(st, f"{vm.prefix}-settings-find-{area}.png", label, w, h)
            if hit:
                break
        if not hit:
            seen = [x for x, b, _ in st.words(f"{vm.prefix}-settings-find-{area}.png") if b[0] > w // 2]
            log(f"settings: {area} not in the sidebar; read there: {seen}")
            failures.append(f"settings: no sidebar entry {area}")
            continue
        x_side = hit[0]
        vm.click(*hit)
        vm.pump(4)
        vm.shot(f"settings-page-{area}")
        png = f"{vm.prefix}-settings-page-{area}.png"
        page = _page_text(st, png, max(200, hit[0] - 70), h - 60)
        bad = [b for b in UNAVAILABLE if st.norm(b) in page]
        bad += [k.group(0) for k in RAW_KEY.finditer(page) if not k.group(0).endswith((".tech", ".txt"))]
        good = not expect or any(st.norm(e) in page for e in expect)
        log(f"settings page {area}: unavailable={bad} values={'yes' if good else 'no'} ({len(page)} chars)")
        if bad:
            failures.append(f"settings {area}: 'not available' or a raw key on the page {bad}")
        if not good:
            failures.append(f"settings {area}: no real values ({expect})")
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


def _switch_left_of(screen, x_text, y0, y1):
    """The control at the start (left, in Arabic) end of a settings row whose texts begin at x_text and fill
    the band y0..y1: from there leftwards, the first group of columns with pixels other than the card's ground
    that is 20-80 points wide (a switch); its centre or None."""
    def px(x, y):
        i = (y * screen.w + x) * 3
        return screen.px[i], screen.px[i + 1], screen.px[i + 2]

    ground = px(max(0, x_text - 6), y0)

    def marked(x):
        ys = [y for y in range(y0, y1) if sum(abs(int(p) - int(q)) for p, q in zip(px(x, y), ground)) > 30]
        return ys

    x, seg, gap, rows = x_text - 8, None, 0, []
    while x > 4:
        ys = marked(x)
        if ys:
            seg = seg or x
            rows += ys
            gap = 0
        elif seg is not None:
            gap += 1
            if gap > 3:
                width = seg - (x + gap)
                if 20 <= width <= 80:
                    return ((seg + x + gap) // 2, (min(rows) + max(rows)) // 2)
                seg, rows, gap = None, [], 0
        x -= 1
    return None


def settings_change(vm, m, pos):
    """A change takes effect and stays: on Notifications the switch of "Do not disturb" (found on the screen)
    is clicked - the switch changes; after Settings is closed and opened again it shows the changed state
    (settingsd stored it); a second click switches it back."""
    st = _screen_text()
    w, h = vm.size

    def open_page(tag):
        vm.move(640, 400)
        vm.pump(1)
        before = vm.shot(f"before-change-{tag}")
        vm.click(*pos["settings"])
        if not app_window(vm, f"change-{tag}", before):
            return None
        vm.pump(3)
        vm.shot(f"change-find-{tag}")
        hit = _sidebar_hit(st, f"{vm.prefix}-change-find-{tag}.png", "الإشعارات", w, h)
        if not hit:
            return None
        vm.click(*hit)
        vm.pump(3)
        vm.move(hit[0], h - 80)  # the pointer off the page (no hover on the switch)
        vm.pump(1)
        screen = vm.shot(f"change-page-{tag}")
        # the row "Do not disturb": its label in the page (left of the sidebar), the switch at the row's start
        words = [(x, b) for x, b, _ in st.words(f"{vm.prefix}-change-page-{tag}.png") if b[0] < hit[0] - 70]
        rows = [b for x, b in words if "الازعاج" in st.norm(x)]
        if not rows:
            return None
        b = min(rows, key=lambda r: r[1])
        # the row: its label and the description under it (words that start within 30 points below)
        band = [wb for _, wb in words if b[1] - 4 <= wb[1] <= b[1] + 30]
        y0, y1 = b[1] - 10, max(wb[3] for wb in band) + 6
        at = _switch_left_of(screen, min(wb[0] for wb in band), y0, y1)
        log(f"settings change ({tag}): row at {b}, switch at {at}")
        return at

    at = open_page("1")
    if not at:
        close_all(vm)
        return ["settings change: the switch of Do not disturb was not found"]
    box = (at[0] - 40, at[1] - 16, at[0] + 40, at[1] + 16)
    a = vm.shot("change-a")
    vm.click(*at)
    vm.pump(2.5)
    vm.move(at[0], h - 80)
    vm.pump(1)
    b = vm.shot("change-b")
    changed = b.diff(a, box)
    close_all(vm)
    at2 = open_page("2")
    if not at2:
        close_all(vm)
        return ["settings change: the switch was not found again"]
    box2 = (at2[0] - 40, at2[1] - 16, at2[0] + 40, at2[1] + 16)
    c = vm.shot("change-c")
    kept, undone = c.diff(b, box2), c.diff(a, box2)
    log(f"settings change: switched {changed:.3f}, after reopening like switched {kept:.3f} / like before {undone:.3f}")
    vm.click(*at2)  # back to the start value
    vm.pump(2)
    close_all(vm)
    failures = []
    if changed < 0.05:
        failures.append("settings change: the switch did not change")
    elif not kept < undone:
        failures.append("settings change: the change did not stay after reopening")
    return failures


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
        failures += store_and_files(vm, m, pos)
        failures += settings_change(vm, m, pos)
    if not failures:
        failures += clipboard(vm, m, pos)
    if not failures:
        failures += accessibility(vm, m)
    if failures:
        log("VMWARE-INPUT: FEHLER " + "; ".join(failures))
        return 1
    log("VMWARE-INPUT: ALLES OK (locked at start, keyboard, pointer, start menu, 5 apps, settings pages, a change that stays, store, files)")
    return 0

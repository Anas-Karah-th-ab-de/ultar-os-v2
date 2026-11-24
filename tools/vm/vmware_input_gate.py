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
    box = window_box(vm.shot("clip-settings-box"), empty)
    if box is None:
        return ["clipboard: Settings' window not found on the screen"]
    vm.click(box[2] - 144, box[1] + 67)  # Settings' search field (top of its side bar, at the start side)
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
    # a second window opens down and to the side of the first (cascade): found on the screen
    box = window_box(vm.shot("clip-files"), typed)
    if box is None:
        close_all(vm)
        close_all(vm)
        return ["clipboard: Files' window not found on the screen"]
    point, field = files_search(box)
    vm.click(*point)
    vm.pump(1)
    before = vm.shot("clip-before-paste")
    vm.key("ctrl+v")
    ok, after = m.wait_for(vm, "clip-pasted", lambda x: x.diff(before, field) > 0.03, 15)
    close_all(vm)
    log(f"clipboard: Files' search field changed {after.diff(before, field):.3f} after Ctrl+V")
    close_all(vm)
    return [] if ok else ["clipboard: Ctrl+V in Files did not paste what Ctrl+C copied in Settings"]


def window_box(screen, before):
    """(left, top, right) of the window that appeared since `before`: its title bar is the first row of
    the work area that differs clearly over a long run (windows are sized from the screen: nothing about
    their place is assumed). A window behind it that only lost the focus changes its title bar far less
    than a new title bar over the ground or over a window's body does. None when no such row exists."""
    w, h = screen.w, screen.h
    a, b = screen.px, before.px
    for y in range(0, h - 60, 2):
        ra, rb = a[y * w * 3:(y + 1) * w * 3], b[y * w * 3:(y + 1) * w * 3]
        if ra == rb:
            continue
        xs = [x for x in range(0, w, 2)
              if abs(ra[x * 3] - rb[x * 3]) + abs(ra[x * 3 + 1] - rb[x * 3 + 1]) + abs(ra[x * 3 + 2] - rb[x * 3 + 2]) > 40]
        if len(xs) * 2 >= w * 0.3:
            return (xs[0], y, xs[-1])
    return None


def files_search(box):
    """Files' search field in its window (toolbar, at the end side in Arabic): the point to click and the
    rectangle that shows typed text."""
    left, top, _ = box
    return (left + 191, top + 63), (left + 84, top + 43, left + 294, top + 83)


def files_list(vm, m, pos):
    """Files in list view (owner: "show as list" hung the window): Ctrl+2 switches, clicks into the items
    (a row, when the place has any) and the window still answers - its search field takes typed text and
    Ctrl+1 switches back."""
    w, h = vm.size
    cx, cy = w // 2, h // 2
    vm.move(cx, cy)
    vm.pump(1)
    before = vm.shot("list-empty")
    vm.click(*pos["files"])
    if not app_window(vm, "list-files", before):
        return ["files list: Files did not open"]
    grid = vm.shot("list-grid")
    vm.key("ctrl+2")
    vm.pump(2)
    for dy in (-120, -80, -40):  # the first rows of the table, or the empty state
        vm.click(cx - 60, cy + dy)
        vm.pump(1)
    vm.pump(2)
    box = window_box(vm.shot("list-box"), before)
    if box is None:
        close_all(vm)
        return ["files list: Files' window not found on the screen"]
    point, field = files_search(box)
    vm.click(*point)
    vm.pump(1)
    typed_before = vm.shot("list-before-type")
    vm.type("ab")
    ok, after = m.wait_for(vm, "list-typed", lambda x: x.diff(typed_before, field) > 0.03, 20)
    log(f"files list: search field changed {after.diff(typed_before, field):.3f} in list view")
    problems = [] if ok else ["files list view: the window stopped answering (typed text did not arrive)"]
    close_all(vm)
    log(f"files list: grid shot kept ({grid.w}x{grid.h})")
    return problems


def status_clusters(screen):
    """The status icons on the bar (Arabic: clock at the start corner, then the status icons, then the
    language chip): runs of columns left of the middle whose light pixels stay within the icon band around
    the bar's centre line (the clock's two lines reach above and below it)."""
    w, h, px = screen.w, screen.h, screen.px
    mid = h - BAR_Y_FROM_BOTTOM

    def light(x, y):
        i = (y * w + x) * 3
        return px[i] + px[i + 1] + px[i + 2] > 540

    runs, start = [], None
    for x in range(0, w // 2):
        inside = any(light(x, y) for y in range(mid - 9, mid + 10))
        outside = any(light(x, y) for y in list(range(h - 54, mid - 12)) + list(range(mid + 13, h - 2)))
        icon = inside and not outside
        if icon and start is None:
            start = x
        if not icon and start is not None:
            if x - start >= 6:
                runs.append((start, x - 1))
            start = None
    merged = []
    for a, b in runs:
        if merged and a - merged[-1][1] <= 6:
            merged[-1] = (merged[-1][0], b)
        else:
            merged.append((a, b))
    return [((a + b) // 2, mid) for a, b in merged]


def quick_settings(vm, m):
    """The quick settings by real clicks (owner: the Wi-Fi symbol and the tiles opened nothing): a click on a
    status icon of the bar opens a sheet above the bar, Esc closes it again."""
    vm.move(vm.size[0] // 2, vm.size[1] // 2)
    vm.pump(1)
    empty = vm.shot("qs-empty")
    icons = status_clusters(empty)
    log(f"quick settings: status icons at {icons}")
    if not icons:
        return ["quick settings: no status icon on the bar"]
    vm.click(*icons[0])
    ok, s = m.wait_for(vm, "qs-open", lambda x: x.diff(empty, work(vm)) > 0.01, 20)
    log(f"quick settings: sheet {s.diff(empty, work(vm)):.3f} of the work area")
    if not ok:
        return ["quick settings: a click on the status icon opened nothing"]
    vm.key("esc")
    ok, _ = m.wait_for(vm, "qs-closed", lambda x: x.diff(empty, work(vm)) < 0.005, 20)
    return [] if ok else ["quick settings: Esc did not close the sheet"]


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
        failures += clipboard(vm, m, pos)
    if not failures:
        failures += files_list(vm, m, pos)
    if not failures:
        failures += quick_settings(vm, m)
    if not failures:
        failures += accessibility(vm, m)
    if failures:
        log("VMWARE-INPUT: FEHLER " + "; ".join(failures))
        return 1
    log("VMWARE-INPUT: ALLES OK (locked at start, keyboard, pointer, start menu, 5 apps, list view, quick settings)")
    return 0

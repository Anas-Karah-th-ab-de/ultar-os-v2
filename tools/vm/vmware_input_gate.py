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


def changed_px(a, b, box):
    """Number of differing pixels inside box (every pixel: a focus ring or one glyph is small)."""
    x0, y0, x1, y1 = box
    n = 0
    for y in range(max(0, y0), min(a.h, y1)):
        row = y * a.w * 3
        for x in range(max(0, x0), min(a.w, x1)):
            i = row + x * 3
            if a.px[i:i + 3] != b.px[i:i + 3]:
                n += 1
    return n


def changed_box(a, b, box):
    """Bounding box of the pixels that differ inside box (every 2nd pixel), or None."""
    x0, y0, x1, y1 = box
    xs, ys = [], []
    for y in range(y0, y1, 2):
        row = y * a.w * 3
        for x in range(x0, x1, 2):
            i = row + x * 3
            if a.px[i:i + 3] != b.px[i:i + 3]:
                xs.append(x)
                ys.append(y)
    if not xs:
        return None
    return (min(xs), min(ys), max(xs), max(ys))


def changes_within(vm, name, before, box, seconds, pixels=12):
    """Seconds until at least `pixels` pixels of box differ from before (None: not within seconds)."""
    start = time.time()
    while time.time() - start < seconds:
        vm.pump(0.2)
        s = vm.shot(name)
        if changed_px(s, before, box) >= pixels:
            return round(time.time() - start, 2), s
    return None, vm.shot(name)


def phone_responds(vm, opened_after, before):
    """The phone's window (it opened opened_after seconds after the click) answers input within 2 s:
    a click on its title gives it the focus, Tab moves the focus ring (drawn by the app), a second Tab
    again. A window that blocks on the call service (no modem, no call service on a PC) or that loses its
    frames shows nothing new and fails here. The no-modem state itself is checked by the phone's tests
    (apps/ultar-phone/tests/states.rs); here: it opens at once without one."""
    problems = []
    if opened_after > 10:
        problems.append(f"phone: the window took {opened_after:.1f} s to open")
    vm.move(vm.size[0] - 2, 2)  # the pointer off the window (no hover)
    vm.pump(1)
    shot = vm.shot("phone-open")
    frame = changed_box(shot, before, work(vm))
    log(f"phone: window at {frame}, opened after {opened_after:.1f} s")
    if frame is None:
        return problems + ["phone: no window found"]
    x0, y0, x1, y1 = frame
    vm.click((x0 + x1) // 2, y0 + 18)  # the title (drawn by the app: ClientHeader)
    vm.move(vm.size[0] - 2, 2)
    vm.pump(1)
    for n in (1, 2):
        start = vm.shot(f"phone-tab{n}-before")
        vm.key("tab")
        took, _ = changes_within(vm, f"phone-tab{n}", start, frame, 2.0)
        log(f"phone: Tab {n} answered after {took} s")
        if took is None:
            problems.append(f"phone: no answer to Tab {n} within 2 s (window frozen)")
            break
    problems += phone_pad_scrolls(vm, frame)
    return problems


def wheel(vm, x, y, down=True, n=5):
    vm.move(x, y)
    for _ in range(n):
        for pressed in (True, False):
            vm.events([{"type": "btn", "data": {"down": pressed, "button": "wheel-down" if down else "wheel-up"}}])
            vm.pump(0.05)


def phone_pad_scrolls(vm, frame):
    """The window a PC gives the phone is lower than the dial pad: the pad scrolls instead of being
    cut off. Tab walks the focus down the pad (keys, call button); the focused control is scrolled
    into view, so the pad's top (heading, notice) moves out of the window. Only where the window is
    that low (screens up to 800 high). The wheel is tried first (it needs the input driver's wheel)."""
    x0, y0, x1, y1 = frame
    w, h = vm.size
    px = x1 - 180 if x1 - x0 > 700 else (x0 + x1) // 2
    strip = ((x0 + x1) // 2, y0 + 45, x1 - 8, y0 + 130)  # the pad's heading (Arabic: right column)
    before = vm.shot("phone-pad-top")
    wheel(vm, px, (y0 + y1) // 2)
    vm.move(w - 2, 2)
    vm.pump(1)
    by_wheel = changed_px(vm.shot("phone-pad-wheel"), before, strip)
    for _ in range(18):
        vm.key("tab")
    vm.pump(1)
    after = vm.shot("phone-pad-scrolled")
    moved = changed_px(after, before, strip)
    log(f"phone: dial pad top changed by the wheel {by_wheel}, by Tab to the lower keys {moved} pixels")
    if moved < 200 and by_wheel < 200 and h <= 800:
        return ["phone: the dial pad does not scroll in a window lower than it (cut off)"]
    return []


def layout_switch(vm, m, pos):
    """Ctrl+Shift and Alt+Shift switch the keyboard layout (the compositor's keymap): the same key typed
    into Settings' search field gives a different character after Ctrl+Shift (Arabic sh -> Latin a), the
    bar's language indicator changes with it, and Alt+Shift switches back (the first character again)."""
    w, h = vm.size
    cx, cy = w // 2, h // 2
    vm.move(cx, cy)
    vm.pump(1)
    empty = vm.shot("kbd-empty")
    vm.click(*pos["settings"])
    if not app_window(vm, "kbd-settings", empty):
        return ["layout: Settings did not open"]
    field = (cx + 120, cy - 212, cx + 360, cy - 172)  # Settings' search field (see clipboard)
    bar = (0, h - 56, w, h)
    clock = (0, h - 56, 140, h)

    def typed(name):
        vm.click(cx + 240, cy - 192)
        vm.pump(0.5)
        vm.key("ctrl+a")
        vm.key("backspace")
        vm.pump(1)
        clear = vm.shot(f"kbd-{name}-clear")
        vm.key("a")
        took, s = changes_within(vm, f"kbd-{name}", clear, field, 3.0)
        return clear, s, took

    problems = []
    clear, first, took1 = typed("first")
    vm.move(cx, cy)
    vm.pump(0.5)
    before = vm.shot("kbd-bar-before")
    vm.key("ctrl+shift")
    # the clock alone must not count: the indicator sits beside it
    rest = (clock[2], h - 56, w, h)
    took_bar, after = changes_within(vm, "kbd-bar-after", before, rest, 3.0)
    indicator = changed_px(after, before, rest)
    _, second, took2 = typed("second")
    vm.key("alt+shift")
    vm.pump(1)
    _, third, took3 = typed("third")
    d12, d13 = changed_px(second, first, field), changed_px(third, first, field)
    log(f"layout: typed a -> {took1}/{took2}/{took3} s, field pixels first/second differ {d12}, "
        f"first/third {d13}, bar indicator pixels {indicator} after {took_bar} s")
    if took1 is None or took2 is None or took3 is None:
        problems.append("layout: a typed key did not arrive in the field")
    elif d12 < 20:
        problems.append("layout: Ctrl+Shift did not change the typed character (same glyph)")
    elif d13 > d12 / 2:
        problems.append("layout: Alt+Shift did not switch back (third glyph unlike the first)")
    if indicator < 12:
        problems.append("layout: the bar's language indicator did not change")
    vm.key("ctrl+a")
    vm.key("backspace")
    close_all(vm)
    return problems


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


def layout_after_restart(vm, m):
    """The input language survives a restart: on the sign-in Ctrl+Shift switches (the chip at the
    bottom changes), the lock screen stores it in the settings service, and after a reset of the
    machine the sign-in shows that language again (not the first one)."""
    w, h = vm.size
    sign_in(vm)
    vm.pump(2)
    bottom = (0, h - 120, w, h)
    first = vm.shot("lang-first")
    vm.key("ctrl+shift")
    took, second = changes_within(vm, "lang-second", first, bottom, 3.0)
    box = changed_box(second, first, bottom)
    log(f"restart layout: chip changed after {took} s at {box}")
    if took is None or box is None:
        return ["restart layout: Ctrl+Shift changed nothing on the sign-in"]
    x0, y0, x1, y1 = box
    chip = (x0 - 4, y0 - 4, x1 + 5, y1 + 5)
    vm.pump(3)  # stored by the lock screen
    mark = len(vm.buf)
    vm.command("system_reset")
    end = time.time() + 300
    while time.time() < end and b"] settled" not in vm.buf[mark:]:
        vm.pump(1)
    ok, _ = m.wait_for(vm, "lang-restart", lambda x: pure_black(x) > 0.6 and x.facts()["mark"] >= 20, 120)
    if not ok:
        return ["restart layout: no lock screen after the restart"]
    sign_in(vm)
    end = time.time() + 20
    after = vm.shot("lang-after")
    while time.time() < end and changed_px(after, second, chip) > changed_px(after, first, chip):
        vm.pump(1)
        after = vm.shot("lang-after")
    like_second, like_first = changed_px(after, second, chip), changed_px(after, first, chip)
    log(f"restart layout: chip after the restart differs {like_second} from the saved one, {like_first} from the first")
    if not like_second < like_first:
        return ["restart layout: after the restart the sign-in shows the first layout, not the saved one"]
    vm.key("alt+shift")  # back to the first (stored too)
    vm.pump(2)
    return []


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
    clicked = time.time()
    vm.click(*phone_tile(*vm.size))
    if not app_window(vm, "app-phone", before, unlike=grid):
        failures.append("phone did not open")
    else:
        failures += phone_responds(vm, time.time() - clicked, before)
    # 5. the screen keeps changing by itself: the bar's clock turns within a minute
    close_all(vm)
    clock = (0, vm.size[1] - 56, 140, vm.size[1])
    first = vm.shot("clock-1")
    ok, _ = m.wait_for(vm, "clock-2", lambda x: x.diff(first, clock) > 0.002, 75)
    if not ok:
        failures.append("the clock did not tick (static screen)")
    # the clipboard and the layout field use positions of Settings' and Files' fields measured at
    # 1280x800 and above; a smaller screen (usb-xga) checks the phone and the restart layout only
    small = vm.size[0] < 1280
    if small:
        log(f"clipboard and typed layout: skipped at {vm.size} (field positions measured at 1280x800)")
    if not failures and not small:
        failures += clipboard(vm, m, pos)
    if not failures and not small:
        failures += layout_switch(vm, m, pos)
    if not failures:
        failures += accessibility(vm, m)
    if not failures:
        failures += layout_after_restart(vm, m)
    if failures:
        log("VMWARE-INPUT: FEHLER " + "; ".join(failures))
        return 1
    log("VMWARE-INPUT: ALLES OK (locked at start, keyboard, pointer, start menu, 5 apps, phone answers and scrolls, layout switch, layout after restart)")
    return 0

"""The steps of `vmware-input.py gate` (only emulated devices through QMP, no test hook).

Screen positions are those of the desktop bar in Arabic (right to left): the bar fills the bottom 56
points; from the right: start (brand mark), all apps, search, then the pinned apps files, browser,
terminal, editor, store, settings (platform/ultar-desktop model/apps.rs). The bar is found from its
brand mark and every window from what it changed on the screen, so the steps do not depend on the
screen's size (1280x800, 1024x768, 1920x1080); positions inside a window are counted from the
window's own corners. Every click is checked on the screen, never assumed."""
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


def park(vm):
    """The pointer off the windows (no hover) and off the work area (its picture is in the screen
    shots): the bar's corner at the end, where the bar has nothing."""
    vm.move(vm.size[0] - 2, vm.size[1] - 2)


def window_frame(vm, name, before):
    """What a window changed of the work area: its frame with its shadow, (x0, y0, x1, y1), or None."""
    park(vm)
    vm.pump(1)
    return changed_box(vm.shot(name), before, work(vm))


# Settings' search field, counted from the top corner at the end of its window's frame (Arabic: the
# right one; the navigation column with the field keeps its width), and Files' search field from the
# top corner at the start (the left one). Measured on the frames found at 1280x800 and 1024x768.
def settings_field(frame):
    x0, y0, x1, y1 = frame
    return (x1 - 166, y0 + 79), (x1 - 300, y0 + 58, x1 - 34, y0 + 100)


def files_field(frame):
    """Files' search field and its toolbar: the field starts 115 points from the window's edge where
    the toolbar has the room and at the edge on a narrow window; the click is where it is in both."""
    x0, y0, x1, y1 = frame
    return (x0 + 160, y0 + 76), (x0 + 22, y0 + 56, x0 + 330, y0 + 98)


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
    frame = window_frame(vm, "clip-settings-frame", empty)
    if frame is None:
        return ["clipboard: Settings' window not found"]
    at, field = settings_field(frame)
    clear = vm.shot("clip-clear")
    vm.click(*at)
    vm.pump(1)
    vm.type("ultar")
    vm.pump(1)
    park(vm)
    vm.pump(0.5)
    typed = vm.shot("clip-typed")
    in_field = changed_px(typed, clear, field)
    log(f"clipboard: Settings at {frame}, typing changed {in_field} pixels of its search field")
    if in_field < 40:
        close_all(vm)
        return ["clipboard: the typed text did not arrive in Settings' search field"]
    vm.key("ctrl+a")
    vm.key("ctrl+c")
    vm.pump(1)
    typed = vm.shot("clip-copied")
    # Settings stays open (it serves the copied text), Files opens above it, as large as Settings
    # and moved down and to the side by a title bar's height (cascade): by how far its frame
    # reaches beyond Settings'
    vm.click(*pos["files"])
    vm.pump(6)
    both = window_frame(vm, "clip-files", typed)
    if both is None:
        close_all(vm)
        close_all(vm)
        return ["clipboard: Files did not open above Settings"]
    dx, dy = max(0, both[2] - frame[2]), max(0, both[3] - frame[3])
    files = (frame[0] + dx, frame[1] + dy, frame[2] + dx, frame[3] + dy)
    at, bar = files_field(files)
    vm.click(*at)
    park(vm)
    vm.pump(1)
    before = vm.shot("clip-before-paste")
    vm.key("ctrl+v")
    took, after = changes_within(vm, "clip-pasted", before, bar, 15.0, pixels=40)
    log(f"clipboard: Files at {files}, its toolbar changed {changed_px(after, before, bar)} pixels "
        f"{took} s after Ctrl+V")
    close_all(vm)
    close_all(vm)
    return [] if took is not None else ["clipboard: Ctrl+V in Files did not paste what Ctrl+C copied in Settings"]


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
    park(vm)
    vm.pump(1)
    shot = vm.shot("phone-open")
    frame = changed_box(shot, before, work(vm))
    log(f"phone: window at {frame}, opened after {opened_after:.1f} s")
    if frame is None:
        return problems + ["phone: no window found"]
    x0, y0, x1, y1 = frame
    vm.click((x0 + x1) // 2, y0 + 18)  # the title (drawn by the app: ClientHeader)
    park(vm)
    vm.pump(1)
    for n in (1, 2):
        start = vm.shot(f"phone-tab{n}-before")
        vm.key("tab")
        took, _ = changes_within(vm, f"phone-tab{n}", start, frame, 2.0)
        log(f"phone: Tab {n} answered after {took} s")
        if took is None:
            problems.append(f"phone: no answer to Tab {n} within 2 s (window frozen)")
            break
    problems += phone_key_click(vm, frame)
    problems += phone_pad_whole(vm, frame)
    if not problems:
        problems += phone_small_window(vm, before)
    return problems


def phone_key_click(vm, frame):
    """A click with the pointer on a key of the dial pad types its digit into the number field (the
    window took the pointer's position 256 times too far out: no click reached a key). The keys are
    counted from the top corner at the end of the window's frame (Arabic: the pad's column is the
    right one, 400 points wide; the middle key of the first rows), the number field is above them."""
    x0, y0, x1, y1 = frame
    number = (x1 - 422, y0 + 180, x1 - 22, y0 + 342)
    before = vm.shot("phone-click-before")
    typed = 0
    for dy in (365, 420, 392):
        for _ in range(2):
            vm.click(x1 - 222, y0 + dy)
        park(vm)
        vm.pump(1)
        typed = changed_px(vm.shot("phone-click"), before, number)
        if typed >= 40:
            break
    log(f"phone: clicks on a key at {(x1 - 222, y0 + dy)} changed {typed} pixels of the number field")
    if typed < 40:
        return ["phone: a click on a key of the dial pad types nothing"]
    return []


# Tab presses that walk the focus once through everything in the phone's window (header, history,
# way, number, the twelve keys)
PHONE_TABS = 28


def wheel(vm, x, y, down=True, n=5):
    vm.move(x, y)
    vm.move(x, y)
    for _ in range(n):
        for pressed in (True, False):
            vm.events([{"type": "btn", "data": {"down": pressed, "button": "wheel-down" if down else "wheel-up"}}])
            vm.pump(0.05)


def phone_pad_whole(vm, frame):
    """The window the phone opens with shows the whole dial pad (it was lower than the pad and cut
    the lower keys and the call button off). The pad is a scroll container that brings the focused
    control into view: Tab walks the focus through every key, and where a key is outside the window
    the pad scrolls and its heading at the top moves. In the opened window nothing may move (the
    heading has no control in it). That the pad does scroll where it is cut off is shown by
    phone_small_window."""
    x0, y0, x1, y1 = frame
    heading = (x1 - 300, y0 + 70, x1 - 40, y0 + 114)
    before = vm.shot("phone-whole")
    text = bright(before, heading)
    moved = 0
    for n in range(PHONE_TABS):
        vm.key("tab")
        vm.pump(0.2)
        moved = changed_px(vm.shot("phone-whole-tab"), before, heading)
        if moved >= 100:
            break
    log(f"phone: window {x1 - x0}x{y1 - y0} with its shadow, heading {text} text pixels, "
        f"{moved} of them moved while Tab walked through the keys")
    if text < 100:
        return [f"phone: no heading above the dial pad at {heading}"]
    if moved >= 100:
        return [f"phone: the dial pad is cut off in the window it opens with (it scrolled at Tab {n + 1})"]
    return []


def phone_small_window(vm, desktop):
    """A small window (Super+Left, Super+Up: a quarter of the screen, lower than the dial pad): the
    pad scrolls instead of being cut off, with the wheel over it and with the focus (Tab to the
    lower keys); its top moves out of the window both times."""
    w, h = vm.size
    wide = vm.shot("phone-before-small")
    vm.key("meta_l+left")
    vm.pump(1)
    vm.key("meta_l+up")
    park(vm)
    vm.pump(2)
    small = vm.shot("phone-small")
    # the quarter at the top left; beside it the empty desktop shows again
    outside = (w // 2 + 40, 60, w - 40, h - 100)
    if changed_px(small, wide, (0, 0, w, h - 60)) < 2000 or changed_px(small, desktop, outside) > 200:
        return ["phone: Super+Left, Super+Up did not make the window a quarter of the screen"]
    # the top of the pad (heading, notice) at the end side of the quarter, below the header and
    # the pages' tabs of a narrow window; no control is there, and it stays inside the pad's column
    # (the 400 points at the end of a window that is not narrow; its scrollbar is at the column's start)
    top = (max(w // 4 + 16, w // 2 - 376), 114, w // 2 - 24, 150)
    over = (w // 2 - 100, (h - 60) // 2 - 80)
    problems = []
    wheel(vm, *over)
    park(vm)
    vm.pump(1)
    by_wheel = changed_px(vm.shot("phone-small-wheel"), small, top)
    wheel(vm, *over, down=False, n=8)
    park(vm)
    vm.pump(1)
    back = vm.shot("phone-small-back")
    left = changed_px(back, small, top)
    by_tab = 0
    for n in range(PHONE_TABS):
        vm.key("tab")
        vm.pump(0.2)
        by_tab = changed_px(vm.shot("phone-small-tab"), back, top)
        if by_tab >= 100:
            break
    log(f"phone: small window {w // 2}x{(h - 60) // 2}: the pad's top moved {by_wheel} pixels by the wheel "
        f"({left} left after the wheel back), {by_tab} by Tab {n + 1}")
    if by_wheel < 100:
        problems.append("phone: the wheel does not scroll the dial pad in a small window")
    elif left >= 100:
        problems.append("phone: the wheel does not scroll the dial pad back")
    if by_tab < 100:
        problems.append("phone: the dial pad does not scroll to the focused key in a small window (cut off)")
    return problems


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
    frame = window_frame(vm, "kbd-settings-frame", empty)
    if frame is None:
        return ["layout: Settings' window not found"]
    at, field = settings_field(frame)
    clock = (0, h - 56, 140, h)

    def typed(name):
        vm.click(*at)
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
# Bluetooth scan (branch build/btscan; run "bt": BT_USB=1 puts the scripted controller
# tools/kern-test/bt-usb.py behind usb-redir on the xHCI, as a controller passed through to a VMware machine).
# Everything from here to the next line of dashes belongs to this block.
# The controller's air: a phone (inquiry result, named by remote name request), a keyboard (extended inquiry
# result), a mouse and a pen (LE advertising, the pen named by its scan response); names as in bt-usb.py.
BT_NAMES = {"phone": "telefon", "keyboard": "tastatur", "mouse": "maus", "pen": "stift"}
# commands of a scan in the controller's log: LE scan parameters, LE scan enable, inquiry, remote name request
BT_SCAN_COMMANDS = ["0x200b", "0x200c", "0x0401", "0x0419"]


def _bt_switch_left_of(screen, x_text, y0, y1):
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


def bluetooth_scan(vm, m, pos):
    """Settings > Bluetooth (opened with the keyboard), then the pointer: the search switch (found on the
    screen) is clicked, the service scans (the controller's log has its inquiry and LE scan commands) and the
    page lists the four devices of the controller's air by name while the search runs."""
    st = _screen_text()
    w, h = vm.size
    box = _open_settings(vm, pos, "bt-settings")
    if not box:
        return ["bluetooth: settings did not open"]
    _goto(vm, "bluetooth", None)
    vm.pump(2)
    screen = vm.shot("bt-page")
    png = f"{vm.prefix}-bt-page.png"
    # the page: the window without its title bar and the sidebar (at the right in Arabic)
    x0, y0, x1, y1 = box[0], box[1] + TITLEBAR, max(box[0] + 100, box[2] - SIDEBAR), box[3]
    words = [(x, b) for x, b, _ in st.words(png) if x0 <= b[0] < x1 and y0 <= b[1] < y1]
    page = st.norm(" ".join(x for x, _ in words))
    bad = [b for b in UNAVAILABLE if st.norm(b) in page]
    if bad:
        close_all(vm)
        return [f"bluetooth: 'not available' on the page {bad}"]
    # the search switch: the lowest row labelled "البحث عن أجهزة" (the state row above it has the same words;
    # the hint under the label has "البحث" without "عن")
    rows = [b for x, b in words if "البحث" in st.norm(x)
            and any(st.norm(y) == "عن" and abs(c[1] - b[1]) < 12 and c[2] <= b[0] + 4 for y, c in words)]
    if not rows:
        close_all(vm)
        return [f"bluetooth: no search switch on the page (no usable controller?); read: {page[:200]}"]
    b = max(rows, key=lambda r: r[1])
    # the row: its label and the note under it (words that start within 30 points below)
    band = [wb for _, wb in words if b[1] - 4 <= wb[1] <= b[1] + 30]
    at = _bt_switch_left_of(screen, min(wb[0] for wb in band), b[1] - 10, max(wb[3] for wb in band) + 6)
    log(f"bluetooth: window {box}, search row at {b}, switch at {at}")
    if not at:
        close_all(vm)
        return [f"bluetooth: the search row at {b} has no switch"]
    vm.click(*at)
    # the list may be longer than the window at 1024x768: each round reads the page as it is and once more
    # after the wheel moved it down (pointer over the page, above the search row)
    found, seen_after = set(), None
    over = ((b[0] + b[2]) // 2, max(y0 + 20, b[1] - 60))
    for n in range(12):
        vm.pump(3)
        for part in ("top", "down"):
            vm.move(*over)
            if part == "down":
                for _ in range(4):
                    vm.events([{"type": "btn", "data": {"down": True, "button": "wheel-down"}}])
                    vm.events([{"type": "btn", "data": {"down": False, "button": "wheel-down"}}])
                    vm.pump(0.2)
                vm.pump(1)
            vm.shot(f"bt-scan-{n}-{part}")
            text = _page_text(st, f"{vm.prefix}-bt-scan-{n}-{part}.png", x1, y1).lower()
            found |= {k for k, v in BT_NAMES.items() if v in text}
        if len(found) == len(BT_NAMES):
            seen_after = 3 * (n + 1)
            break
    log(f"bluetooth: devices on the page {sorted(found)} after {seen_after} s")
    failures = []
    missing = sorted(set(BT_NAMES) - found)
    if missing:
        failures.append(f"bluetooth: devices not listed: {missing}")
    try:
        with open(vm.bt_log, errors="replace") as f:
            ctl = f.read()
    except OSError:
        ctl = ""
    silent = [c for c in BT_SCAN_COMMANDS if f"BTEMU cmd {c}" not in ctl]
    log(f"bluetooth: controller got {len(re.findall('BTEMU cmd', ctl))} commands, scan commands missing: {silent}")
    if silent:
        failures.append(f"bluetooth: the service did not send {silent}")
    close_all(vm)
    return failures
# ---------------------------------------------------------------------------------------------------------
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
    vm.move(vm.size[0] // 2, vm.size[1] // 2)
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
    # Bluetooth scan (only in the run with the scripted controller; that run ends with it)
    if getattr(vm, "bt_log", None):
        failures += bluetooth_scan(vm, m, pos)
        if failures:
            log("VMWARE-INPUT: FEHLER " + "; ".join(failures))
            return 1
        log("VMWARE-INPUT: ALLES OK (bluetooth: locked at start, keyboard, pointer, start menu, a scan from Settings lists the devices found)")
        return 0
    # 4. every system app opens its window: Settings, Files, Store, Browser from the bar, Phone from
    #    the app grid
    for name in ("settings", "files", "store", "browser"):
        vm.move(vm.size[0] // 2, vm.size[1] // 2)
        vm.pump(1)
        before = vm.shot(f"before-{name}")
        vm.click(*pos[name])
        if not app_window(vm, f"app-{name}", before):
            failures.append(f"{name} did not open")
        close_all(vm)
    vm.move(vm.size[0] // 2, vm.size[1] // 2)
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
    if not failures:
        failures += settings_pages(vm, m, pos)
        failures += settings_maximized(vm, m, pos)
        failures += store_and_files(vm, m, pos)
        failures += settings_change(vm, m, pos)
    if not failures:
        failures += clipboard(vm, m, pos)
    if not failures:
        failures += layout_switch(vm, m, pos)
    if not failures:
        failures += files_list(vm, m, pos)
    if not failures:
        failures += quick_settings(vm, m)
    if not failures:
        failures += accessibility(vm, m)
    if not failures:
        failures += layout_after_restart(vm, m)
    if failures:
        log("VMWARE-INPUT: FEHLER " + "; ".join(failures))
        return 1
    log("VMWARE-INPUT: ALLES OK (locked at start, keyboard, pointer, start menu, 5 apps, settings pages, maximized, "
        "a change that takes effect and survives a restart, store, files, phone answers, its dial pad whole and "
        "scrolling in a small window, clipboard, layout switch, list view, quick settings, layout after restart)")
    return 0

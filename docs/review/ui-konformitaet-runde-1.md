# UI conformance audit – Ultar OS v2 (Prüfer Oberfläche)

Independent tester, 2026-11-24, HEAD `ce287d2` (repo read-only; working copy `uiaudit/repo`).
Plan sources: 26 boards `design/boards/project/*.dc.html` (873 clickable elements, 176 actions), `design/tokens.toml`,
`docs/DESKTOP-SPEC.md`, `docs/ui/CLICKS.md`, `docs/ui/ICONS.md`, `docs/APP-RULES.md`, all `locales/*.ftl`.

## Verdict

**The planned UI is not implemented.** On HEAD only two layers of the plan exist in new code:
1. the toolkit `platform/ultar-ui` (bricks, menus, toasts, sheets = boards *States*, *StatesLight*, *Menus*, *Bricks*);
2. the compositor title bar (`platform/ultar-compositor/src/decor.rs` + `state/grabs.rs:225`, actions `win.min/max/close`).

No desktop shell, quick settings, calendar sheet, overview, lock/login screen, projection switcher, installer, settings
pages, store, files, browser chrome or phone app exists as product code built from `ultar-ui` (STATUS.md agrees).
The only composed screen (display settings) is a toolkit demo in `platform/ultar-ui/tests/common/mod.rs` – not product
code, and it deviates from the board. The legacy `platform/ultar-shell` (old DRM UI; quick.rs, app_phone.rs …) hard-codes
colours (`rgb(…)` literals) and is slated for replacement (STATUS P5/P6) – **not counted** as implementing any board.

Overall (OK = 1, DIFFERENT = ½): **18 %** of 849 comparable clickable elements – 95 OK, 116 DIFFERENT, 638 MISSING,
0 DEAD CLICK (every implemented click had its effect; none panicked). Product surfaces 0–8 %, toolkit boards 58–81 %.

## Method (automated and reusable – CHANGES.patch)

- `tools/design-conformance/board_inventory.py`: renders every board headless (Chrome + playwright, fonts from
  `assets/fonts`, network blocked) at 1x → `shots/boards/*.png`; extracts every clickable (`data-act`), icon, landmark,
  text run with box, colours, font, outline → `inventory.json`.
- `static_checks.py`: icons (path data board vs `ultar-ui` icon table + ICONS.md), tokens (both token files identical,
  every board colour a token), click list (board ↔ CLICKS.md per action and name), translations (ar/en key parity, no
  English or placeholders in Arabic), coverage list (every CLICKS action has a status and a proving test).
- `make_manifest.py` → `design/conformance/states.tsv`: every brick × state × theme of *States*/*StatesLight* with its box
  and the board's pixel colour at sample points (fill, border, focus ring, ground); `--check` detects stale manifests.
- `platform/ultar-ui/tests/design_conformance.rs`: rebuilds each brick with the toolkit (board texts, Arabic, dark + light,
  state applied by real pointer/keyboard input), compares size ±2 px and colours (fill ±4, border/ring ±40), writes the
  rebuilt board PNG; plus BiDi check: numbers/units as message arguments stay LTR in Arabic (passes).
- `platform/ultar-ui/tests/design_conformance_clicks.rs`: drives `menu.open`, `menu.item` (disabled, submenu, RTL side),
  `select.option`, `tabs.tab` (click, arrows in reading direction, Home/End), `sheet.close` (Esc, scrim, focus return),
  `toast.action` (click, Ctrl+Enter), `toast.close` (button, Esc, timeout); measures *Menus*/*Bricks* sizes;
  keyboard-only paths and the accessibility tree (toast buttons found via `a11y_tree`).
- `platform/ultar-compositor/tests/design_conformance.rs`: title-bar height, button order + mirroring, geometry.
- Ratchet: `design/conformance/known-differences.txt` (72 lines) and `known-static.txt` (6). A new difference fails; a
  listed one that disappears also fails ("remove it") – the lists only shrink.
- `run.sh` runs all of it (board side skipped with a notice without Chrome); new CI job `design-conformance` uploads
  the side-by-side PNGs. Mutation proof: shifting the `hover` token by 6 steps fails 4+ bricks. Suite green on HEAD
  (static 0 new; ultar-ui 2 + 9 tests; compositor 3 tests); clippy -D warnings, fmt, ultar-check clean.
- `matrix.py` → `MATRIX.md`: full matrix board → element → status → implementation file (849 rows).

Verified OK: token files identical; every board colour is a token; 115/116 board icons exist in ultar-ui with
**identical path data**; CLICKS.md matches every board (count and action); ultar-ui catalogs ar/en/de complete.

## Totals per board

| Board | Size | Dir | Clickable | OK | DIFFERENT | MISSING | DEAD | % complete* | Arabic texts in catalogs | Icons | Regions |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Account | 1440×900 | rtl | 57 | 0 | 3 | 54 | 0 | 3% | 42/79 | 51 | 10 |
| Bricks | 1280×800 | ltr | 12 | 2 | 10 | 0 | 0 | 58% | 0/0 | 4 | 4 |
| CalendarSheet | 1440×900 | rtl | 63 | 0 | 0 | 63 | 0 | 0% | 10/23 | 22 | 17 |
| DesktopBottom | 1440×900 | ltr | 28 | 0 | 3 | 25 | 0 | 5% | 0/0 | 29 | 3 |
| DesktopTop | 1440×900 | rtl | 40 | 0 | 6 | 34 | 0 | 8% | 9/19 | 39 | 5 |
| Installer | 1280×800 | rtl | 25 | 0 | 0 | 25 | 0 | 0% | 14/61 | 27 | 15 |
| InstallerDisk | 1280×800 | rtl | 22 | 0 | 0 | 22 | 0 | 0% | 12/48 | 14 | 5 |
| InstallerDrivers | 1280×800 | rtl | 26 | 0 | 0 | 26 | 0 | 0% | 23/49 | 33 | 48 |
| InstallerSummary | 1280×800 | rtl | 23 | 0 | 0 | 23 | 0 | 0% | 14/38 | 15 | 3 |
| Lock | 1440×900 | rtl | 6 | 0 | 0 | 6 | 0 | 0% | 7/12 | 10 | 5 |
| Login | 1440×900 | rtl | 9 | 0 | 0 | 9 | 0 | 0% | 1/5 | 7 | 2 |
| Main | 1440×900 | rtl | 40 | 0 | 6 | 34 | 0 | 8% | 11/20 | 40 | 4 |
| Menus | 1280×800 | rtl | 16 | 3 | 13 | 0 | 0 | 59% | 10/20 | 12 | 7 |
| Overview | 1440×900 | rtl | 24 | 0 | 0 | 24 | 0 | 0% | 5/8 | 26 | 2 |
| PhoneDial | 1280×800 | rtl | 36 | 0 | 3 | 33 | 0 | 4% | 9/21 | 27 | 7 |
| PhoneInCall | 1280×800 | rtl | 12 | 0 | 3 | 9 | 0 | 12% | 1/16 | 17 | 4 |
| PhoneIncoming | 1280×800 | rtl | 7 | 0 | 3 | 4 | 0 | 21% | 3/10 | 10 | 2 |
| Projection | 900×320 | rtl | 4 | 0 | 0 | 4 | 0 | 0% | 3/8 | 1 | 2 |
| QuickSettings | 1440×900 | rtl | 38 | 0 | 0 | 38 | 0 | 0% | 14/20 | 38 | 2 |
| SettingsDisplay | 1440×900 | rtl | 59 | 0 | 3 | 56 | 0 | 3% | 54/78 | 47 | 7 |
| SettingsDisplayLight | 1440×900 | ltr | 56 | 0 | 3 | 53 | 0 | 3% | 0/0 | 47 | 4 |
| SettingsDrivers | 1440×900 | rtl | 54 | 0 | 3 | 51 | 0 | 3% | 35/63 | 63 | 49 |
| States | 1760×1184 | rtl | 72 | 45 | 27 | 0 | 0 | 81% (+12 n/a) | 9/34 | 48 | 119 |
| StatesLight | 1760×1184 | rtl | 72 | 45 | 27 | 0 | 0 | 81% (+12 n/a) | 9/34 | 48 | 119 |
| Store | 1280×800 | ltr | 27 | 0 | 3 | 24 | 0 | 6% | 0/0 | 40 | 15 |
| SurfaceStates | 2496×640 | rtl | 21 | 0 | 0 | 21 | 0 | 0% | 11/35 | 20 | 12 |
| **all** | | | 849 | 95 | 116 | 638 | 0 | 18.0% | | | |

\* OK counts 1, DIFFERENT ½ (works, but does not look/behave as drawn), MISSING/DEAD 0.

"Arabic texts in catalogs" = board Arabic text runs found verbatim in some `locales/ar.ftl` (readiness of translations
for the missing surfaces; LTR boards DesktopBottom/SettingsDisplayLight/Store/Bricks are English by design).

## Coverage by area

| Area | Boards | Status on HEAD | File reference |
|---|---|---|---|
| Toolkit bricks (all states, dark/light, Arabic) | States, StatesLight | 45/72 OK each, 27 DIFFERENT, 12 n/a (error column off-board) | platform/ultar-ui/src/widgets/* |
| Menus, select list, tooltip | Menus | 3 OK, 13 DIFFERENT (widths) | ui/menus.rs, widgets/menu.rs, select.rs |
| Tabs, toast, sheet, scrollbar | Bricks | 2 OK, 10 DIFFERENT | widgets/tabs.rs, ui/toasts.rs, ui/modal.rs |
| Window title bar (win.*) | Main, DesktopTop/Bottom, Settings*, Account, Store, Phone* | works, geometry DIFFERENT | compositor decor.rs, state/grabs.rs |
| Desktop shell: bar, dock, desk icons, kbd, status, clock | Main, DesktopTop, DesktopBottom | MISSING | (in progress: w5/shell) |
| Quick settings, calendar + notifications | QuickSettings, CalendarSheet | MISSING (backend only: ultar-settingsd seeds) | (in progress: w5/shell) |
| Overview, app grid (`shell.apps`), search | Overview | MISSING; **no board for the app grid** | (in progress: w5/launch) |
| Lock, login, projection, keep-dialog | Lock, Login, Projection, SettingsDisplay | MISSING (compositor has only the `Role::Lock` layer) | (in progress: w5/shell) |
| Settings pages | SettingsDisplay(+Light), SettingsDrivers, Account | MISSING (test demo only; backend in ultar-settingsd) | – |
| Installer | Installer, InstallerDisk, InstallerDrivers, InstallerSummary | MISSING (legacy text installer in kernel only) | – |
| Store + its states | Store, SurfaceStates | MISSING | – |
| Phone | PhoneDial, PhoneIncoming, PhoneInCall | MISSING (legacy ultar-shell app_phone.rs, not ultar-ui) | – |
| Files / browser windows | Main, DesktopBottom | MISSING | – |

## Toolkit differences found (DIFFERENT – details in run output `states-differences.tsv`)

| # | Where | Board | ultar-ui | Severity |
|---|---|---|---|---|
| T1 | List item with sublabel (`ListItem::sublabel`) | row 56 high; hover/press/focus fill and ring cover both lines | rect **32** high: hover fill and **focus ring cut through the sublabel** (shots/States-side.png, bottom row) | major |
| T2 | Navigation list item | chevron `›` at the end; 16-pt status icon | no chevron; 24-pt icon | minor |
| T3 | Text field focused/pressed | control_border stays + blue focus ring; pressed = text-colour border | border turns **gold (accent_text)** when focused/typing | major (focus colour rule §3: blue next to gold) |
| T4 | Text field: LTR value in RTL field | value `ultar-pc` starts at the left | right-aligned | minor |
| T5 | Text field loading | spinner at the end of the field | **no loading state** (`set_loading` → error "node has no such state") | major (MISSING state) |
| T6 | Checkbox loading | box kept checked | box replaced by a spinner | minor |
| T7 | Disabled primary button / checkbox border | whole widget at 0.40 opacity (border 104,94,55) | border drawn at ~0.6 (150,130,61) | minor |
| T8 | Slider disabled | filled track (105,94,55) | (126,117,81) | minor |
| T9 | Focus ring of switch, slider | ring 2 px **outside the 48×32 / whole control** | ring hugs the 24-high track / only the thumb | minor |
| T10 | Button widths (primary, quiet; all states) | 74.3 / 85.4 | 71 / 83 (−2.4…−3.3 px; same font – text advance differs) | minor |
| T11 | Menu width | File menu 224, context 224, submenu 160 | 144, 202, 144 (content width, no minimum) | minor |
| T12 | Submenu placement | 4-pt gap beside the parent | overlaps the parent by 4 | minor |
| T13 | Menu owner while open | owner button shows the open/hover fill | no open state on the owner | minor |
| T14 | Tab strip | 327.7 wide | 368 (wider tab padding) | minor |
| T15 | Toast with action | CLICKS: closes itself after motion.toast (4 s) | never times out when it has an action | spec conflict – decide and write into DESKTOP-SPEC |
| T16 | Toast action | runs on release like every button | runs on **press** (cannot be cancelled by moving away) | minor |
| T17 | Title bar buttons (compositor) | 32×32, radius 8, 4-pt gap and inset, y=4 | 40×40 flush, square hover fill | minor |
| T18 | Display demo screen (test only) | "main display" = badge/button, never off (§13 #64); number badge; HDR = disabled switch; header buttons "find displays / show numbers"; keep-dialog | uses a **switch** for main display, no number badge, HDR as text "غير متاح", bottom bar Detect/Revert/Apply | informational (not product) |

Correct (OK): fills/borders of plain, danger, iconbutton, select, switch off/on in all six states; hover/pressed overlays;
focus ring colour (info) on buttons, select, checkbox; disabled opacity of fills; loading spinners on buttons/select/
iconbutton; heights 32/48; RTL mirroring; select/tabs keyboard; sheet focus return; Esc everywhere; BiDi of numbers.

## Board defects found (the plan itself)

B1 *States/StatesLight*: the "error" column (7th) lies at x < 0, outside the artboard – no visual spec for error states
of 12 bricks. B2 *QuickSettings*: switch accessible names contain the state ("بلوتوث: مُطفأ" + aria-checked) → double
announcement. B3 *DesktopTop*: the clock button has no accessible name "التقويم والإشعارات" (name = the date text).
B4 icon `mark` used on boards is not in ultar-ui. B5 no artboard for the **app grid** (`shell.apps`), the window menu
(Alt+Space), notifications toasts on the desktop, and **no compact/phone-size boards** for shell, settings, store
(only the Bricks bottom sheet) – the phone requirement cannot be checked. B6 settingsd Arabic `paper.choice.letter`
= "Letter" (untranslated). B7 CLICKS.md lists elements in a different order than the DOM (harmless; checks use per-action counts).

## Prioritized fix list

Blocking (the release cannot claim "UI per plan"):
1. Build the product surfaces – 638 MISSING clickables: desktop shell (bar/dock/desk, status + clock sheets), lock/login,
   overview + app grid + search, projection + keep dialog, settings pages (Display, Drivers, Account), installer (4),
   store (+5 states), files, browser chrome, phone (3). Each must pass `run.sh` (extend `states.tsv`-style manifests per board).
2. T1 list item with sublabel: height 56 (size.row_min) and state fill/focus ring over the whole row.
3. T3 text field focus = control border + info ring (not gold); T5 add loading state to text fields.
4. Add missing boards (B5: app grid, compact sizes) – without them the launcher and phone cannot be verified.
Major/minor next: T15 decide toast timeout (spec vs toolkit), T9 focus ring geometry, T7/T8 disabled opacity applied
to the whole widget, T11/T12/T13 menus, T17 title bar geometry, T2/T4/T6/T10/T14/T16, board defects B1–B4, B6.

## Surfaces being built right now – what their deliverable must cover

Audited on HEAD only (nothing of w5/shell or w5/launch is on HEAD). Their result must cover these board elements
(names = accessible names on the boards; behaviour = CLICKS.md) and must be added to `coverage.toml` with a test:

- **Main** (1440×900, rtl): regions: nav “الأماكن”, nav “الشريط”, section “الطرفية”, section “الملفات”
  - `win.min` ×2: تصغير
  - `win.max` ×2: تكبير
  - `win.close` ×2: إغلاق
  - `desk.icon` ×2: المجلد الرئيسي · ذاكرة USB
  - `shell.trash` ×1: سلة المحذوفات
  - `shell.drive` ×1: ذاكرة USB
  - `shell.overview` ×1: نظرة عامة
  - `shell.apps` ×1: كل التطبيقات
  - `shell.search` ×1: بحث
  - `shell.app` ×6: الملفات (يعمل) · المتصفح · الطرفية (يعمل) · محرر النصوص · متجر Ultar · الإعدادات
  - `shell.kbd` ×1: لغة الإدخال: العربية
  - `shell.status` ×1: الإعدادات السريعة
  - `shell.clock` ×1: التقويم والإشعارات: الأحد 22 تشرين الثان
- **DesktopTop** (1440×900, rtl): regions: nav “الأماكن”, nav “الشريط”, nav “المرسى”, section “الطرفية”, section “الملفات”
  - `win.min` ×2: تصغير
  - `win.max` ×2: تكبير
  - `win.close` ×2: إغلاق
  - `desk.icon` ×2: المجلد الرئيسي · ذاكرة USB
  - `shell.overview` ×1: نظرة عامة
  - `shell.appmenu` ×1: الملفات
  - `shell.clock` ×1: الأحد 22 تشرين الثاني · 16:52
  - `shell.kbd` ×1: لغة الإدخال: العربية
  - `shell.status` ×1: الإعدادات السريعة
  - `shell.apps` ×1: كل التطبيقات
  - `shell.app` ×6: الملفات (يعمل) · المتصفح · الطرفية (يعمل) · محرر النصوص · متجر Ultar · الإعدادات
  - `shell.drive` ×1: ذاكرة USB
  - `shell.trash` ×1: سلة المحذوفات
- **DesktopBottom** (1440×900, ltr): regions: nav “Bar”, section “Ultar Browser”, tablist “Tabs”
  - `win.min` ×1: Minimize
  - `win.max` ×1: Maximize
  - `win.close` ×1: Close
  - `shell.trash` ×1: Trash
  - `shell.overview` ×1: Overview
  - `shell.apps` ×1: All apps
  - `shell.search` ×1: Search
  - `shell.app` ×6: Files · Browser (running) · Terminal · Text editor · Ultar Store · Settings
  - `shell.kbd` ×1: Input language: English
  - `shell.status` ×1: Quick settings
  - `shell.clock` ×1: Calendar and notifications: Sun 22 Nov 1
- **QuickSettings** (1440×900, rtl): regions: dialog “الإعدادات السريعة”, nav “الشريط”
  - `qs.wifi` ×1: واي فاي: KM-Office
  - `qs.wifi.list` ×1: واي فاي: اختيار
  - `qs.wired` ×1: الشبكة السلكية: متصلة · 1 Gbit/s
  - `qs.bluetooth` ×1: بلوتوث: مُطفأ
  - `qs.bluetooth.list` ×1: بلوتوث: اختيار
  - `qs.toggle` ×4: وضع الطيران: غير مفعّل · الضوء الليلي: حتى 06:10 · الوضع الداكن: مفعّل · عدم الإزعاج: غير مفعّل
  - `qs.vpn` ×1: VPN: مُطفأ
  - `qs.vpn.list` ×1: VPN: اختيار
  - `qs.powermode` ×1: وضع الطاقة: متوازن
  - `qs.powermode.list` ×1: وضع الطاقة: اختيار
  - `qs.mesh` ×1: الترابط: جهازان
  - `qs.mesh.list` ×1: الترابط: اختيار
  - `qs.volume` ×1: مستوى الصوت
  - `qs.output` ×1: جهاز إخراج الصوت: مكبّرات الصوت
  - `qs.brightness` ×1: السطوع
  - `qs.battery` ×1: البطارية 87% · متوازن
  - `qs.assistant` ×1: المساعد
  - `qs.shot` ×1: لقطة شاشة
  - `qs.settings` ×1: الإعدادات
  - `qs.lock` ×1: قفل
  - `qs.power` ×1: خيارات الطاقة
  - `shell.trash` ×1: سلة المحذوفات
  - `shell.drive` ×1: ذاكرة USB
  - `shell.overview` ×1: نظرة عامة
  - `shell.apps` ×1: كل التطبيقات
  - `shell.search` ×1: بحث
  - `shell.app` ×6: الملفات · المتصفح · الطرفية · محرر النصوص · متجر Ultar · الإعدادات
  - `shell.kbd` ×1: لغة الإدخال: العربية
  - `shell.status` ×1: الإعدادات السريعة
  - `shell.clock` ×1: التقويم والإشعارات: الأحد 22 تشرين الثان
- **CalendarSheet** (1440×900, rtl): regions: columnheader “الأحد”, columnheader “الأربعاء”, columnheader “الاثنين”, columnheader “الثلاثاء”, columnheader “الجمعة”, columnheader “الخميس”, columnheader “السبت”, dialog “التقويم والإشعارات”, grid “تشرين الثاني 2026”, nav “الشريط”
  - `note.dnd` ×1: عدم الإزعاج
  - `note.clear` ×1: مسح الكل
  - `note.action` ×5: تحديث النظام: إعادة التشغيل الآن · تحديث النظام: لاحقاً · ذاكرة USB: فتح · ذاكرة USB: إخراج · الجداول: فتح
  - `note.dismiss` ×3: إخفاء الإشعار: تحديث النظام جاهز · إخفاء الإشعار: تم توصيل ذاكرة USB · إخفاء الإشعار: اكتمل تثبيت «الجداول»
  - `cal.month` ×2: الشهر السابق · الشهر التالي
  - `cal.day` ×35: 31 تشرين الأول · 1 تشرين الثاني · 2 تشرين الثاني · 3 تشرين الثاني · 4 تشرين الثاني · 5 تشرين الثاني · 6 تشرين الثاني · 7 تشرين الثاني …
  - `cal.event` ×2: 17:30اجتماع فريق التصميم · 20:00موعد طبيب الأسنان
  - `shell.trash` ×1: سلة المحذوفات
  - `shell.drive` ×1: ذاكرة USB
  - `shell.overview` ×1: نظرة عامة
  - `shell.apps` ×1: كل التطبيقات
  - `shell.search` ×1: بحث
  - `shell.app` ×6: الملفات · المتصفح · الطرفية · محرر النصوص · متجر Ultar · الإعدادات
  - `shell.kbd` ×1: لغة الإدخال: العربية
  - `shell.status` ×1: الإعدادات السريعة
  - `shell.clock` ×1: التقويم والإشعارات: الأحد 22 تشرين الثان
- **Overview** (1440×900, rtl): regions: main “نظرة عامة”, nav “الشريط”
  - `overview.search` ×1: اكتب للبحث في التطبيقات والإعدادات والمل
  - `overview.workspace` ×3: مساحة العمل 1 · مساحة العمل 2 · مساحة عمل جديدة
  - `overview.window` ×3: الملفات – المستندات · الطرفية · الإعدادات – الشاشات
  - `overview.close` ×3: إغلاق النافذة: الملفات – المستندات · إغلاق النافذة: الطرفية · إغلاق النافذة: الإعدادات – الشاشات
  - `shell.trash` ×1: سلة المحذوفات
  - `shell.drive` ×1: ذاكرة USB
  - `shell.overview` ×1: نظرة عامة
  - `shell.apps` ×1: كل التطبيقات
  - `shell.search` ×1: بحث
  - `shell.app` ×6: الملفات (يعمل) · المتصفح · الطرفية (يعمل) · محرر النصوص · متجر Ultar · الإعدادات (يعمل)
  - `shell.kbd` ×1: لغة الإدخال: العربية
  - `shell.status` ×1: الإعدادات السريعة
  - `shell.clock` ×1: التقويم والإشعارات: الأحد 22 تشرين الثان
- **Lock** (1440×900, rtl): regions: div “الحالة”, list “الإشعارات”, main “شاشة القفل”, section “التحكّم بالوسائط”
  - `lock.unlock` ×1: فتح القفل
  - `lock.media` ×3: المقطع السابق · تشغيل · المقطع التالي
  - `lock.sos` ×1: طوارئ
  - `lock.power` ×1: إيقاف التشغيل الطارئ
- **Login** (1440×900, rtl): regions: main “تسجيل الدخول”, radiogroup “المستخدمون”
  - `login.user` ×2: أنس · ليلى
  - `login.password` ×1: كلمة مرور أنس
  - `login.show` ×1: إظهار كلمة المرور
  - `login.submit` ×1: تسجيل الدخول
  - `login.method` ×1: استخدام الرمز PIN
  - `shell.kbd` ×1: لغة الإدخال: العربية
  - `login.access` ×1: تسهيلات الاستخدام
  - `qs.power` ×1: خيارات الطاقة
- **Projection** (900×320, rtl): regions: dialog “وضع العرض”, radiogroup “وضع العرض”
  - `projection.mode` ×4: هذا الجهاز فقط · تكرار · توسيع · الشاشة الثانية فقط
- **SettingsDisplay** (1440×900, rtl): regions: nav “أقسام الإعدادات”, progressbar “الوقت المتبقّي”, radiogroup “الشاشة المحدَّدة”, radiogroup “وضع العرض”, section “الإعدادات”
  - `win.min` ×1: تصغير
  - `win.max` ×1: تكبير
  - `win.close` ×1: إغلاق
  - `display.keep.more` ×1: أحتاج وقتاً أكثر
  - `display.revert` ×1: تراجع
  - `display.keep` ×1: احتفاظ
- **PhoneIncoming** (1280×800, rtl): regions: alertdialog “مكالمة واردة من ليلى حداد”, section “الهاتف”
  - `win.min` ×1: تصغير
  - `win.max` ×1: تكبير
  - `win.close` ×1: إغلاق

**w5/launch (app launcher)**: `shell.apps` ×6 ("كل التطبيقات"/All apps → app grid, alphabetical – no board exists, B5),
`shell.app` ×36 (start or raise; several windows → preview; middle click new window; right click: window list, New
window, quick actions, Pin/Unpin, Details in Store, Quit; drag to reorder; file drop opens with the app),
`shell.search` ×5 + `overview.search` (Overview with caret in the search field), `store.launch` ×11 (start installed app).
Also every dock item's running indicator ("(يعمل)" in the name) and Arabic/English names as on Main/DesktopBottom.

## Deliverables

- `REPORT.md` (this file), `MATRIX.md` (board → element → status → file, 849 rows + totals).
- `shots/`: `boards/*.png` (all 26 board renders at 1x); `States-side.png`, `StatesLight-side.png` (board | toolkit |
  red diff); `Menus-file-menu-side.png`, `Menus-context-submenu-side.png`, `Bricks-side.png`,
  `SettingsDisplay(-Light)-card-side.png` (board vs test demo); `<Board>-side.png` for every missing surface.
- `CHANGES.patch` (`patch -p1` from repo root, tests/tools only, no product code): tools/design-conformance/*,
  design/conformance/*, ultar-ui tests `design_conformance*.rs`, compositor test `design_conformance.rs`, CI job.

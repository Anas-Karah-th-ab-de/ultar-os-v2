# Brand – names, logo and where they appear

KM Code Craft · Ultar OS v2 · 2026-11-24 · belongs to [DESKTOP-SPEC.md](DESKTOP-SPEC.md) §4, §5.1

One rule: **nobody writes the product name, the company, the website, the contact address, the
developer's name or the mark by hand.** Everything comes from one source and `ultar-check brand`
fails when it does not.

## 1. The facts

| Fact | Value | Rust | Key (through `ultar-i18n`) |
|---|---|---|---|
| Product | Ultar OS | `ultar_brand::PRODUCT`, `product!()` | `brand.product` |
| Company / brand | KM Code Craft | `ultar_brand::VENDOR`, `vendor!()` | `brand.vendor` |
| Website | https://code-craft.tech | `ultar_brand::WEBSITE`, `website!()` | `brand.website` |
| Public contact | info@code-craft.tech | `ultar_brand::CONTACT_EMAIL`, `CONTACT_URL` | `brand.contact` |
| Developer | محمد أنس قرة محمد · Mohamad Anas Kara Mohamad | `DEVELOPER_AR`, `DEVELOPER`, `text::developer(lang)` | `brand.developer` |
| Machine identifier | `ultar` | `ultar_brand::PRODUCT_ID` | – |
| Crate `authors` | `KM Code Craft <https://code-craft.tech>` | `MANIFEST_AUTHORS` | – |

The names are never translated and never transliterated: "Ultar OS" and "KM Code Craft" are written
in Latin letters in Arabic text too. No address, telephone number, legal form or year belongs to the
brand data; none may be added without the owner.

## 2. The single source

| What | Where |
|---|---|
| Facts, macros for `concat!` | `system/core/ultar-brand/src/lib.rs` |
| Product mark (path, sizes, clear space) | `system/core/ultar-brand/src/mark.rs` |
| Company logo (proportions, sizes, bitmaps with feature `bitmaps`) | `system/core/ultar-brand/src/logo.rs`, pictures in `assets/brand/` (`scripts/brand-assets.py` writes the `.rgba` files) |
| Brand texts, Arabic first | `system/core/ultar-brand/locales/{ar,en,de}.ftl` (`brand.*`) |
| `os-release`, user agent, USB strings, package maintainer | `system/core/ultar-brand/src/release.rs`; `rootfs/etc/os-release` is a checked copy |

The crate is `no_std` without dependencies: the UEFI loader and the kernel use it directly
(`ultar_brand::text::lookup(lang, key)`), every other program gets all `brand.*` keys from
`ultar_i18n::Translator::new` (the embedded catalogs include them). `system.name` and `system.vendor`
of the base catalog are older names of the same facts and are checked against them.

Brand keys: `brand.product`, `brand.vendor`, `brand.website`, `brand.contact` (facts) ·
`brand.developer`, `brand.by`, `brand.developed_by`, `brand.boot.subline` ·
`brand.about.{name,version,build,vendor,developer,website,contact,licences}`, `brand.about.licence.system` ·
`brand.crash.kernel`, `brand.crash.report.title`, `brand.crash.report.send` ·
`brand.signer.vendor`, `brand.signer.other` · `brand.label.mark`, `brand.label.logo` · `brand.error.*`.

## 3. Two pictures, two jobs

**Product mark** – two open arcs, the sign of Ultar OS (overview button, lock screen, boot, About).
Stroke drawing in the 24-unit box: `M12 3a9 9 0 1 0 9 9M12 8a4 4 0 1 0 4 4`.
Drawn by the icon pipeline of `ultar-ui` as `IconId::Mark` (the check compares the path with
`ultar_brand::mark::PATH`).

**Company logo** – the circuit cloud with the letters and the name line, the sign of KM Code Craft
(boot splash, About, installer welcome). Bitmap only: `assets/brand/logo.png` 520 × 218, picture alone
`mark-circuit.png` 364 × 327, app icon `icon-192.png`.

| Rule | Product mark | Company logo |
|---|---|---|
| Colour | token `accent_text` of the current theme (dark `#E3C55A`, light `#785B00`); nothing else | as drawn; never recoloured |
| Background | any surface of the theme (the token keeps the contrast in dark and light) | **dark only**: `ultar_brand::logo::PLATE` (`#0B0D11`) or a plate of that colour – the grey name line (`#B4B3B4`) is unreadable on light surfaces |
| Stroke | token `size.stroke` (1.5 at 24), scaled with the size by the icon pipeline | – |
| Minimum size | 16 logical points (`mark::MIN_SIZE` = token `size.icon_small`) | 160 px wide (`logo::MIN_WIDTH`); smaller: picture alone ≥ 32 px, or the name as text |
| Sizes | bar 24 · lock/login 48 · boot, installer welcome, About 96 (`mark::BAR_SIZE`, `SCREEN_SIZE`, `HERO_SIZE`) | `logo::fit(max_w, max_h)`: proportions kept, never above 520 × 218 |
| Clear space | a quarter of the edge length on every side (`mark::clear_space`) | a quarter of the height on every side (`logo::clear_space`) |
| Right-to-left | never mirrored (`mark::MIRRORED = false`); it stands at the *start* side, so on the right in Arabic | never mirrored; centred, or at the start side |
| Screen reader | name `brand.label.mark` | name `brand.label.logo` |

Do: draw the mark from the path at the pixel size needed · put the product name next to it as text
(`brand.product`, weight 600) · keep the clear space · use the logo once per screen at most.

Don't: stretch, rotate, fill, outline, shadow or animate either picture · scale a bitmap up · draw the
mark from a bitmap · put the logo on a light surface · recolour the mark with `accent` (the fill gold
is for the one main action) · write "ULTAR OS" in capitals outside the text-mode word mark of the
kernel (`product_caps!()`, bitmap font) · translate or abbreviate the names · put the developer's
name anywhere but About, the kernel `version` line and the README.

## 4. The places

Status on `main` after this change. "Branch" = not on `main`; the owner of that surface makes the call listed.

| Place | Status | What it shows / which call |
|---|---|---|
| UEFI loader messages (`kernel/ultar-loader/src/msg.rs`) | fixed | every line starts with `product!()`; tested |
| Kernel start line, console title, `version`, `uname`, `/proc/version` | fixed | `product_caps!()` · `vendor!()` · `developer!()` |
| Kernel boot picture and start page (`startseite.rs`) | fixed | company logo above the word mark (`Canvas::logo`, area-scaled bitmap, 360 px), word mark `product_caps!()`, sub line `brand.boot.subline` |
| Kernel text installer, language step | fixed | company logo 220 px under the languages when the card has room |
| Kernel stop / panic text | fixed | `brand.crash.kernel` (now with the contact address), same text for trap, halt and panic |
| Kernel installer header, welcome, target, progress, done | fixed | `product!()` / `product_caps!()` / `vendor!()` |
| Boot splash of the Linux-based image (`platform/ultar-shell/src/brand.rs`) | legacy, in baseline | company logo + product name; replace by `ultar_brand::logo::bitmap()` + `PRODUCT` when the legacy shell goes |
| Login and lock screen | branch `wip/desktop-shell` | mark 48 (`IconId::Mark`, `accent_text`) + `brand.product` beside it; **replace key `lock.brand` by `brand.product`**; label `brand.label.mark` |
| Bar: overview button | branch `wip/desktop-shell` | `IconId::Mark` 24, never mirrored, at the start; no text |
| Overview | branch `wip/desktop-shell` | no second mark (the button is the mark); empty state may show mark 96 + `brand.product` |
| Settings › About | branch `wip/settings-installer` | mark 96, then rows `brand.about.name` → `brand.product` · `brand.about.version` · `brand.about.build` · `brand.about.vendor` → `brand.vendor` · `brand.about.developer` → `brand.developer` · `brand.about.website` → `brand.website` (link) · `brand.about.contact` → `brand.contact` (`CONTACT_URL`) · `brand.about.licences` → `brand.about.licence.system` + third-party list; version and build from the installed release (`/etc/os-release` `VERSION_ID`, `BUILD_ID`), never a constant |
| Installer (graphical): welcome, writing, done | fixed (2026-11-28) | welcome: mark 96 + `installer.welcome` + company logo 240 (`BrickKind::Logo`, dark plate in the light theme); the logo also while the system is written and on the last page |
| Desktop background | fixed (2026-11-28) | system picture "ultar-noir": ground of the theme, company logo in the middle of the work area (a third of the shorter side, at most 420), quiet (half strength) – as in the first desktop |
| Lock screen cover | fixed (2026-11-28) | company logo 200 above the clock |
| App grid (start menu) footer | fixed (2026-11-28) | `IconId::Mark` + `brand.product` at the start, the power button at the end |
| Settings › About | fixed (2026-11-28) | mark 96, `brand.product`, company logo 220, then the rows |
| Installer (graphical): summary | branch `wip/settings-installer` | welcome: mark 96 + `installer.welcome`; company logo below on a dark plate, or `brand.by`; summary: `brand.product` + version to be installed, `brand.vendor`; all sentences keep "Ultar OS" spelled as the fact (checked in every `*.ftl`) |
| Store header | boards only | "Ultar Store" is the app's own name; verified publisher line `brand.signer.vendor`, others `brand.signer.other` |
| `/etc/os-release` | added | `rootfs/etc/os-release` = `ultar_brand::release::OS_RELEASE`; image builds append `PRETTY_NAME`, `VERSION_ID`, `VARIANT_ID` (`scripts/build-initramfs.sh`, kern-test media); a Rust image builder calls `release::write_os_release` |
| Package / driver signer names | text added, display on branches | key signed by the company: `brand.signer.vendor`; any other: `brand.signer.other` + the publisher's own name; `.deb` `Maintainer`: `release::PACKAGE_MAINTAINER` (three scripts fixed) |
| Window title / default app icon | rule | a window without a title shows its app name, never the product name; an app without an icon shows the generic app icon, never the mark or the logo |
| Virtual machine name | fixed | `scripts/run-qemu.sh -name` from `rootfs/etc/os-release` |
| USB device strings | fixed | `release::USB_MANUFACTURER`, `USB_PRODUCT` (`ultar-init`) |
| Updater user agent | added | `ultar-update/<version> (Ultar OS; KM Code Craft; +https://code-craft.tech)` from `release::user_agent`; other network clients do the same with their own name |
| Crash report (user space) | text added | title `brand.crash.report.title`, last line `brand.crash.report.send`; no reporter exists yet |
| README header, crate `authors` | fixed / checked | README names company, website, contact, developer; `authors` compared in every manifest |

Legacy code that still writes the names itself is listed in `tools/check-baseline.toml` under
`rule = "brand"` (49 literals: `platform/ultar-shell`, `platform/ultar-wm`, `apps/ultar-web`,
`apps/ultar-office`, `system/services/ultar-sshd`, `ultar-llm`, `ultar-apps`, `external/ultar-telegram`,
`server/cloud-e2e`). The number only goes down.

## 5. The check

`cargo run -p ultar-check -- brand` (part of `all`) fails on: a brand term in a string literal outside
the source · brand catalogs whose languages differ in keys · a `brand.*` key in another catalog · a
name spelled differently in any catalog · another mail address than the public contact in scripts,
configuration or documents · `rootfs/etc/os-release`, a manifest `authors` or the `mark` of the icon
table differing from the source. Details: [integration/ultar-check.md](integration/ultar-check.md).

## 6. Open, needs the owner

1. **System version and build number have no single source.** Root manifest 0.6.0 (this is what the
   image build writes as `VERSION_ID`), new crates 0.7.0, kernel 0.4.0 (printed by the kernel), test
   disk "0.2"; no build identifier exists. About and the installer summary need one.
2. **Product mark and company logo** – the split of §3 follows the boards (arcs) and the existing
   splash (company logo). Confirm, or name one of them the only sign.
3. **Company logo exists only as a bitmap for dark backgrounds.** A vector file and a variant for
   light surfaces are needed (About and installer in the light theme use a dark plate until then).
4. **Gold differs**: tokens `#D4AF37` / `#E3C55A`, kernel console and installer `#C9A227` / `#E6C454`,
   legacy splash `#E9D68A`. The kernel should take the token values.
5. **Capitals "ULTAR OS"** in the kernel's text mode: kept as the word mark where no picture can be
   drawn; drop it if the name must always be "Ultar OS".
6. Minimum sizes and clear space of §3 are set here for the first time; adjust if the brand has rules.

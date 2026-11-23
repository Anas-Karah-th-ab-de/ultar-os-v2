# Fonts in this directory – where each file comes from

Every file is shipped **unmodified** (no subsetting, no renaming, no conversion), as the SIL
Open Font License asks for fonts with reserved names. The table is checked by
`platform/ultar-ui` (`src/text/fonts.rs`, test `fonts_load_and_names_match_the_files`): family
name, weight and "not a variable font" are read from the files themselves.

## Added on 2026-11-23 (wave 3)

Source for all eight files: the official distribution repository of the Noto project,
<https://github.com/notofonts/notofonts.github.io>, commit
`c6f674cf56243b4c0b650134a9020e047eab702e` (2026-11-22), obtained with
`git clone --depth 1 --filter=blob:none --sparse` and a sparse checkout of exactly these
paths. Unhinted static TTF instances (the toolkit's rasteriser does not hint and does not
apply variation axes). Licence: SIL Open Font License 1.1, full text in `OFL.txt` (copied
from `fonts/LICENSE` of the same commit; "Copyright 2022 The Noto Project Authors").

| File | Path in the repository | Version | Bytes | SHA-256 | Used for |
|---|---|---|---|---|---|
| `NotoNaskhArabic-Regular.ttf` | `fonts/NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-Regular.ttf` | Version 2.021 | 158580 | `c34fdbd98af4dbc45ca192a23d2eeb77032add83086f5fe32957d23b3f36b221` | Arabic text 400 |
| `NotoNaskhArabic-Medium.ttf` | `fonts/NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-Medium.ttf` | Version 2.021 | 157872 | `3befc6c9270aa30b6332cd90c5eeb0c9cf801f5e146fca317704585e55c6981c` | Arabic text 500 |
| `NotoNaskhArabic-Bold.ttf` | `fonts/NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-Bold.ttf` | Version 2.021 | 158172 | `380ac34b249d336e1a43c21053df440594e1e3db8f75fabd4d85b649ca1ea4c6` | Arabic text 600/700 |
| `NotoKufiArabic-Regular.ttf` | `fonts/NotoKufiArabic/unhinted/ttf/NotoKufiArabic-Regular.ttf` | Version 2.110 | 139236 | `e245bdc293b07d903603054f88746e8e51d9ff0bfd88905b0d9842198732d2b1` | Arabic titles 400/500 |
| `NotoKufiArabic-Bold.ttf` | `fonts/NotoKufiArabic/unhinted/ttf/NotoKufiArabic-Bold.ttf` | Version 2.110 | 138700 | `a2688019b048aedbc6a232d4332de5a02d938432e356ed694d62de0ae0d6fa1d` | Arabic titles 600/700 |
| `NotoSansArabic-Bold.ttf` | `fonts/NotoSansArabic/unhinted/ttf/NotoSansArabic-Bold.ttf` | Version 2.013 | 141852 | `f4cb79f842a07d1bcf597879574358fb7010f73761396d1d0f2445dfedb4d2ca` | Arabic inside fixed-width text 600/700 |
| `NotoSansMono-Regular.ttf` | `fonts/NotoSansMono/unhinted/ttf/NotoSansMono-Regular.ttf` | Version 2.014 | 388660 | `87f8ce0522a6c99b743ee5fc75b4073cfdd575639119672828b7b9944b65b4f4` | fixed-width 400/500 |
| `NotoSansMono-Bold.ttf` | `fonts/NotoSansMono/unhinted/ttf/NotoSansMono-Bold.ttf` | Version 2.014 | 387484 | `1100772b2f79c102402a1011df5e2226517d0f40ac553a25fb12fdbad8c11b85` | fixed-width 600/700 |

Copyright lines stored in the files (name record 0):

- NotoNaskhArabic: Copyright 2022 The Noto Project Authors (https://github.com/notofonts/arabic)
- NotoKufiArabic: Copyright 2019-2022 Google LLC. All Rights Reserved.
- NotoSansArabic: Copyright 2022 The Noto Project Authors (https://github.com/notofonts/arabic)
- NotoSansMono: Copyright 2022 The Noto Project Authors (https://github.com/notofonts/latin-greek-cyrillic)

## Present before wave 3 (origin not recorded at the time)

These four files came with the first import of the repository (`LIZENZ.txt`). Their bytes do
not equal any file of the commit above (they are older releases, hinted with ttfautohint);
the exact source commit is unknown. They were left untouched because `platform/ultar-shell`,
`platform/ultar-wm` and `kernel/ultar-kern` (install) read them too. Version and licence are
taken from the files' own name records.

| File | Version (name record 5) | Bytes | SHA-256 | Licence | Used for |
|---|---|---|---|---|---|
| `NotoSans-Regular.ttf` | Version 2.004 | 512672 | `89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d` | SIL OFL 1.1 (Noto, Google LLC) | Latin, digits, symbols 400/500 |
| `NotoSans-Bold.ttf` | Version 2.004 | 515752 | `e83493c945848ecd4a9ad0f6d19164541a0d3e23a9c952304a00a46e00272ac5` | SIL OFL 1.1 (Noto, Google LLC) | Latin, digits, symbols 600/700 |
| `NotoSansArabic-Regular.ttf` | Version 2.005 | 244072 | `504d7407d86875acf7d04dfaa0fd7524d0b8797723bc4aa18022f29db25b0b6e` | SIL OFL 1.1 (Noto, Google LLC) | Arabic inside fixed-width text 400/500 (also the old shell) |
| `DejaVuSansMono.ttf` | Version 2.37 | 343140 | `c805f9436dbc268644c1d9584f01a601a653e028e08fd74b9b949f6cf8304d88` | Bitstream Vera Fonts licence + public-domain DejaVu changes | not used by ultar-ui any more; still used by ultar-shell, ultar-wm, kernel install |

Open point: replace the three older Noto files by the unhinted instances of one recorded
commit (the same repository has them) in a change of their own – it alters every English
snapshot and the old shell's rendering, so it was not mixed into this delivery.

## Sizes

Added: 1670556 bytes (8 files). Whole directory: 3286192 bytes (12 font files).
Embedded in `ultar-ui` (11 files, everything except DejaVu Sans Mono): 2943052 bytes; before
wave 3 it embedded 4 files with 1615636 bytes.

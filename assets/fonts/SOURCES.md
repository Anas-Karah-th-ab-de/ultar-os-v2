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
| `NotoNaskhArabic-Bold.ttf` | `fonts/NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-Bold.ttf` | Version 2.021 | 158172 | `380ac34b249d336e1a43c21053df440594e1e3db8f75fabd4d85b649ca1ea4c6` | Arabic text 700 |
| `NotoKufiArabic-Regular.ttf` | `fonts/NotoKufiArabic/unhinted/ttf/NotoKufiArabic-Regular.ttf` | Version 2.110 | 139236 | `e245bdc293b07d903603054f88746e8e51d9ff0bfd88905b0d9842198732d2b1` | Arabic titles 400 |
| `NotoKufiArabic-Bold.ttf` | `fonts/NotoKufiArabic/unhinted/ttf/NotoKufiArabic-Bold.ttf` | Version 2.110 | 138700 | `a2688019b048aedbc6a232d4332de5a02d938432e356ed694d62de0ae0d6fa1d` | Arabic titles 700 |
| `NotoSansArabic-Bold.ttf` | `fonts/NotoSansArabic/unhinted/ttf/NotoSansArabic-Bold.ttf` | Version 2.013 | 141852 | `f4cb79f842a07d1bcf597879574358fb7010f73761396d1d0f2445dfedb4d2ca` | Arabic inside fixed-width text 600/700 |
| `NotoSansMono-Regular.ttf` | `fonts/NotoSansMono/unhinted/ttf/NotoSansMono-Regular.ttf` | Version 2.014 | 388660 | `87f8ce0522a6c99b743ee5fc75b4073cfdd575639119672828b7b9944b65b4f4` | fixed-width 400/500 |
| `NotoSansMono-Bold.ttf` | `fonts/NotoSansMono/unhinted/ttf/NotoSansMono-Bold.ttf` | Version 2.014 | 387484 | `1100772b2f79c102402a1011df5e2226517d0f40ac553a25fb12fdbad8c11b85` | fixed-width 600/700 |

Copyright lines stored in the files (name record 0):

- NotoNaskhArabic: Copyright 2022 The Noto Project Authors (https://github.com/notofonts/arabic)
- NotoKufiArabic: Copyright 2019-2022 Google LLC. All Rights Reserved.
- NotoSansArabic: Copyright 2022 The Noto Project Authors (https://github.com/notofonts/arabic)
- NotoSansMono: Copyright 2022 The Noto Project Authors (https://github.com/notofonts/latin-greek-cyrillic)

## Added and replaced on 2026-11-23 (wave 4, review round 3)

Source: the same official repository, <https://github.com/notofonts/notofonts.github.io>, commit
`682229039c5a594f247890d94e3fb497be8d58dd` (2026-11-23), sparse checkout of exactly these paths,
unhinted static TTF instances, files unmodified. Licence: SIL OFL 1.1 (`OFL.txt`, unchanged).
Why: the token weights 500 and 600 were drawn as 400 and 700 (review m6); the toolkit now
embeds the real Medium and SemiBold faces.

The files come from **two** commits of that repository: the eight wave-3 files from
`c6f674cf…` (table above), the eight wave-4 files from `68222903…` (this table). The font
versions are the same in both commits for every family (Naskh 2.021, Kufi 2.110, Noto Sans
Arabic 2.013, Noto Sans Mono 2.014; Noto Sans 2.015 exists only in the wave-4 set), so the
weights of one family match; the SHA-256 of each file is recorded in its own table.

| File | Path in the repository | Version | Bytes | SHA-256 | Used for |
|---|---|---|---|---|---|
| `NotoSans-Regular.ttf` (replaced) | `fonts/NotoSans/unhinted/ttf/NotoSans-Regular.ttf` | Version 2.015 | 431364 | `f3961a9cde016d41a4879aecda1474d3a36d6bf54fa0e4643de029cc2248b0e8` | Latin, digits, symbols 400 |
| `NotoSans-Medium.ttf` (new) | `fonts/NotoSans/unhinted/ttf/NotoSans-Medium.ttf` | Version 2.015 | 431176 | `220e74546aee1b9a17cd50025dd917a064d0224e653f562287446c2328e4160b` | Latin 500 |
| `NotoSans-SemiBold.ttf` (new) | `fonts/NotoSans/unhinted/ttf/NotoSans-SemiBold.ttf` | Version 2.015 | 431500 | `a7af8d4a11e8e49fea9a19786a6469210b512cd4061413e8036bdd2425722795` | Latin 600 |
| `NotoSans-Bold.ttf` (replaced) | `fonts/NotoSans/unhinted/ttf/NotoSans-Bold.ttf` | Version 2.015 | 432376 | `87cb2d84472a7d66da659ee47b6cdb9552326e8c128245231f191b6ac72529d9` | Latin 700 |
| `NotoSansArabic-Regular.ttf` (replaced) | `fonts/NotoSansArabic/unhinted/ttf/NotoSansArabic-Regular.ttf` | Version 2.013 | 142140 | `bd86ca02f087d7f3c3788ba458fb6b73744c7639ed276b8d870dba6def6c40d0` | Arabic inside fixed-width text 400/500 |
| `NotoNaskhArabic-SemiBold.ttf` (new) | `fonts/NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-SemiBold.ttf` | Version 2.021 | 157868 | `85c6a3c50b19179fe26f550d4e9a658cd8f0e74b25816e6da46b2ba8cdbc7673` | Arabic text 600 |
| `NotoKufiArabic-Medium.ttf` (new) | `fonts/NotoKufiArabic/unhinted/ttf/NotoKufiArabic-Medium.ttf` | Version 2.110 | 138328 | `43948b63c2562f1acfccf3adfe5ba84164cf55bd20729fad93499e0448406946` | Arabic titles 500 |
| `NotoKufiArabic-SemiBold.ttf` (new) | `fonts/NotoKufiArabic/unhinted/ttf/NotoKufiArabic-SemiBold.ttf` | Version 2.110 | 138688 | `ef9c440ceac067b117ef18d2729aecdaf7307f8e8c44a27776fb9b7911d3221f` | Arabic titles 600 |

The three replaced files keep their names and family names ("Noto Sans", "Noto Sans Arabic"),
so `platform/ultar-shell`, `platform/ultar-wm` and `kernel/ultar-kern` (install) read them
unchanged; they are the unhinted 2.015 / 2.013 releases instead of the older hinted 2.004 /
2.005 ones. With this the wave-3 open point ("replace the three older Noto files") is done.
The "Used for" column of the wave-3 table is updated to the faces now embedded (Naskh 600 =
SemiBold, Kufi 500/600 = Medium/SemiBold).

## Present before wave 3 (origin not recorded at the time)

`DejaVuSansMono.ttf` (Version 2.37, 343140 bytes, SHA-256
`c805f9436dbc268644c1d9584f01a601a653e028e08fd74b9b949f6cf8304d88`, Bitstream Vera Fonts
licence + public-domain DejaVu changes, see `LIZENZ.txt`) came with the first import. It is not
used by ultar-ui; ultar-shell, ultar-wm and the kernel installer still use it. The three Noto
files of that import (NotoSans Regular/Bold 2.004, NotoSansArabic Regular 2.005, hinted, origin
unknown) were replaced in wave 4, see above.

## Sizes

Whole directory: 4317136 bytes (17 font files). Embedded in `ultar-ui` (16 files, everything
except DejaVu Sans Mono): 3973996 bytes; wave 3 embedded 11 files with 2943052 bytes, before
wave 3 it was 4 files with 1615636 bytes. Added in wave 4: 1297560 bytes (5 new files); the
three replaced files are 266616 bytes smaller than the old ones (unhinted).

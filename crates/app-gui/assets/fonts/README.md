# Fonts

| File | Font | Licence | Used for |
|---|---|---|---|
| `Inter-Regular.otf` | Inter 3.x (Rasmus Andersson and the Inter Project Authors) | SIL Open Font License 1.1 — `Inter-LICENSE.txt` | Interface and report text |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono | SIL Open Font License 1.1 — `JetBrainsMono-LICENSE.txt` | Numbers in report tables, monospaced text |
| `DejaVuSans.ttf` | DejaVu Sans | Bitstream Vera / DejaVu licence — `DejaVu-LICENSE.txt` | Fallback for mathematical and geometric symbols |
| `LiberationSans-Regular.ttf`, `LiberationSerif-Regular.ttf` | Liberation Sans and Liberation Serif 2.1 (Red Hat; digitized data Google) | SIL Open Font License 1.1 — `Liberation-LICENSE.txt` | Plot text in PNG figures (metric-compatible with the Helvetica and Times that PDF and EPS figures name), and serif plot titles on screen |
| `AnovaCraftMarks.ttf` | AnovaCraft Marks, derived from Inter's combining macron and circumflex by `build_marks.py` | SIL Open Font License 1.1 (as Inter) | Bars and hats over the preceding letter (X̄, λ̂), which egui cannot position itself |

Their licences, with those of everything else the app bundles, are in
`THIRD-PARTY-NOTICES.txt` (Help ▸ About AnovaCraft ▸ Open-source licences;
`scripts/build-notices.py` lists the font licence files). The fonts are embedded in the application (`src/theme.rs`; the PNG writer in `stat-viz/src/raster.rs`).

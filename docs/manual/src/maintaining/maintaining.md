# Keeping the manuals current

AnovaCraft has two manuals, both mdBooks in the repository that change in
the same pull request as the code they describe, so they never fall behind
a release:

- this **Reference Manual** (`docs/manual`), the detailed one for power
  users: every option, report section, method and script function;
- the **Quick Guide** (`docs/guide`), for people new to the app or to
  statistics: one page per menu describing every item in plain language,
  plus a first walk-through, a "which analysis do I need?" chooser and a
  glossary. Each entry links to the Reference Manual page with the
  details. Its pictures (named `guide-*`) are in `docs/guide/src/images`.

## The rule

Any change a user can see updates the manual page that describes it, in the
same pull request. That includes:

- a new analysis, report section, option, dialog setting, menu item or file
  format: add or extend its page;
- a change to something already finished, such as a renamed menu item, a
  new default, a different layout or a removed option: correct every page
  that mentions it, and replace or re-mark any screenshot it makes wrong;
- a feature leaving the [Coming soon](../analyses/coming-soon.md) page: move
  it to its own page;
- a menu item added, renamed or moved: its entry on the Quick Guide's page
  for that menu (`docs/guide/src/menus/<menu>.md`), in **bold**, in a sentence
  or two of plain language.

`FEATURES.md` (the terse list of what the app does) changes in the same pull
request too; the manual is the explanation, `FEATURES.md` the inventory.

## How it is enforced

- **CLAUDE.md** states the rule, so every coding session follows it.
- The **pull request template** has a manual checkbox.
- **CI** builds both manuals on every pull request and checks them: a
  missing page or include, a link to a page that does not exist, an image
  that is not in `images/`, a picture whose proportions are not a window's
  (5:4 to 2:1) or whose size is forced, or a screenshot marker missing from
  the checklist fails the build. It also warns when a pull request
  changes user-facing code (`crates/app-gui`, `crates/app-model`,
  `crates/stat-script`) or `FEATURES.md` without touching either manual,
  `docs/formulas.md` or `docs/scripting.md`. A warning is a prompt to check,
  not a failure: internal refactors need no manual change.
- The test `embedded_manuals_match_their_sources` fails when either book
  changed without its PDF being rebuilt (see below).
- The GUI test `quick_guide_names_every_menu_item` (run with the other
  tests) opens every menu of the running app and fails if the Quick Guide
  does not name one of its items.

## Where things go

| What | Where |
|---|---|
| Pages | `docs/manual/src/<part>/<page>.md` |
| Table of contents | `docs/manual/src/SUMMARY.md` |
| Quick Guide pages | `docs/guide/src/menus/<menu>.md` and the pages beside it |
| Screenshots | `docs/manual/src/images/<id>.png`, rendered by `scripts/capture-manual-shots.sh` (see [Screenshots](screenshots.md)) |
| Formula function reference | `docs/formulas.md` (included here, not copied) |
| Script API reference | `docs/scripting.md` (included here, not copied) |

## The PDFs in the app

Help ▸ Quick guide and Help ▸ Reference manual open PDFs of the two books
that are built into the app (`crates/app-gui/assets/manuals`, embedded with
`include_bytes!`, so every build carries them). After changing either book,
`docs/formulas.md` or `docs/scripting.md`, rebuild them:

```sh
./scripts/build-manual-pdfs.sh
```

It builds both books with mdBook, prints each to a PDF with a title page,
linked contents, in-document chapter links and a bookmark outline
(`scripts/book-pdf.mjs`, which needs Node with Playwright's Chromium), and
re-encodes the screenshots as JPEG at most 1100 pixels wide (quality 0.65;
`MANUAL_IMAGE_WIDTH` and `MANUAL_JPEG_QUALITY` change that). It also writes
`docs-stamp.txt`, a hash of every source file of the books
(`scripts/manual-stamp.mjs`). Commit the three files it writes.

The test `embedded_manuals_match_their_sources` (run with the other tests)
computes the same hash and fails when the Markdown changed but the PDFs were
not rebuilt. When two branches both rebuilt the PDFs, resolve the merge by
running the script again on the merged sources.

## Previewing

Install mdBook once, then serve the book with live reload:

```sh
cargo install mdbook
mdbook serve docs/manual --open   # the Reference Manual
mdbook serve docs/guide --open    # the Quick Guide
```

`scripts/check-manual.sh` runs the same checks as CI.

## Style

- Write for someone using the app, not building it: what to click, what they
  get, what it means.
- Use the app's own labels in **bold**, and menu paths as Menu ▸ Item.
- In the Quick Guide, write for someone who has never taken a statistics
  course: what the item is for and when to reach for it, in everyday words;
  leave options and formulas to the Reference Manual.
- Name the published method and, where useful, the sample dataset to try.
- Follow the project's clean-room naming: standard statistical terms only.

## Knowledge pack for chat assistants

`knowledge-pack/` holds Markdown files that a user can give any web chat
assistant so it can answer questions about AnovaCraft: an `llms.txt` index, a
short `00-essentials.md`, and seven files covering the overview and glossary,
menus and features, this manual, the validation record, design conventions, a
code map built from doc comments, and the development guide, plus
`anovacraft-knowledge-pack.md`, which holds all seven in one self-contained
file for assistants that take a single upload. The files are
generated from FEATURES.md, both manuals, `docs/validation.md`, CLAUDE.md,
ROADMAP.md and the crates' doc comments; never edit them by hand.

```sh
python3 scripts/build-knowledge-pack.py          # regenerate
python3 scripts/build-knowledge-pack.py --check  # is it current?
python3 scripts/build-knowledge-pack.py --zip knowledge-pack.zip  # also zip it
```

Regenerate the pack when a release is prepared, or whenever the
documentation has changed a good deal.

## Licence notices

`THIRD-PARTY-NOTICES.txt` at the repository root holds the copyright notices
and licence texts of everything the shipped programs contain that
AnovaCraft's authors did not write: the bundled fonts, icons and lexicon, and
every Rust crate compiled into the app, `anovacraft-run` or the web viewer
(on macOS, Windows and WebAssembly). The app and `anovacraft-run` embed it,
the macOS and Windows build scripts copy it beside the program, and Publish
as web app… writes it beside the viewer in the published folder. It is
generated from Cargo.lock and the crates' own licence files (crates that ship
none get the standard text from `scripts/licences/`); never edit it by hand.

```sh
python3 scripts/build-notices.py          # regenerate after changing dependencies
python3 scripts/build-notices.py --check  # is it current?
```

The test `third_party_notices_match_cargo_lock` fails when Cargo.lock changes
without the file being rebuilt. A new bundled font, icon set or data file gets
an entry in the script's `ASSETS` list, with its licence file in the
repository.

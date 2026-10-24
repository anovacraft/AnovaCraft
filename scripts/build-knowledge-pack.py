#!/usr/bin/env python3
"""Build the AnovaCraft knowledge pack for web chat assistants.

    python3 scripts/build-knowledge-pack.py          # writes knowledge-pack/
    python3 scripts/build-knowledge-pack.py --check  # fails if knowledge-pack/ is stale

The pack is a handful of Markdown files a user can upload to any web-based
language model (or add to its project knowledge) so it can answer questions
about AnovaCraft's use, design, code and development. It is generated from
the repository's own documents and doc comments, never from full source:

  llms.txt                         index of the files below (llmstxt.org layout)
  00-essentials.md                 short version for assistants with small limits
  01-overview-and-glossary.md
  02-menus-and-features.md         FEATURES.md and the Quick Guide
  03-reference-manual.md           the Reference Manual, includes resolved
  04-validation.md                 how each statistical routine is verified
  05-design-and-gui-conventions.md
  06-architecture-and-code-map.md  crates, modules and public items with their doc lines
  07-development-guide.md          rules, roadmap, maintenance pages, CI, scripts
  anovacraft-knowledge-pack.md     01–07 in one self-contained file

Run it after documentation or public API changes; it needs only Python 3.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "knowledge-pack"
REPO_URL = "https://github.com/anovacraft/AnovaCraft"

PREAMBLE = (
    "> From the AnovaCraft knowledge pack, generated from the repository by "
    "`scripts/build-knowledge-pack.py`. AnovaCraft is an open-source statistics application written in Rust. "
    "Answer from this text: it does not depend on any other file. Paths such as `crates/...` or `docs/...` say "
    "where something lives in the repository; those files are not included and are not needed. Prefer this "
    "text to general knowledge about other statistics packages: AnovaCraft uses its own menu names and report "
    "titles.\n"
)


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def git_head() -> str:
    try:
        return subprocess.run(
            ["git", "log", "-1", "--format=%h (%cs)"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def demote(text: str, levels: int) -> str:
    """Push every Markdown heading down `levels` levels (outside code fences)."""
    out, fence = [], False
    for line in text.splitlines():
        if line.lstrip().startswith("```"):
            fence = not fence
        if not fence and re.match(r"#{1,6} ", line):
            hashes = len(line) - len(line.lstrip("#"))
            line = "#" * min(6, hashes + levels) + line[hashes:]
        out.append(line)
    return "\n".join(out)


def strip_images(text: str) -> str:
    """Images do not travel with the pack; keep their alt text."""
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", lambda m: f"(Picture: {m.group(1)})" if m.group(1) else "", text)
    return re.sub(r"<img[^>]*>", "", text)


def unlink(text: str) -> str:
    """Relative links point at files the reader does not have; keep the words."""
    return re.sub(r"\[([^\]]+)\]\((?!https?://)[^)]*\)", r"\1", text)


def resolve_includes(text: str, base: Path) -> str:
    def inc(m: re.Match[str]) -> str:
        return read((base / m.group(1).strip()).resolve())

    return re.sub(r"\{\{#include ([^}]+)\}\}", inc, text)


def book_pages(book: Path) -> list[tuple[str, Path]]:
    """(title, path) of each page in an mdBook's SUMMARY.md, in reading order."""
    src = book / "src"
    pages = []
    for title, link in re.findall(r"\[([^\]]+)\]\(([^)]+\.md)\)", read(src / "SUMMARY.md")):
        path = src / link
        if path.exists():
            pages.append((title, path))
    return pages


def book_text(book: Path, levels: int) -> str:
    parts = []
    for _, path in book_pages(book):
        page = resolve_includes(read(path), path.parent)
        parts.append(demote(unlink(strip_images(page)), levels).strip())
    return "\n\n".join(parts)


def section(markdown: str, heading: str) -> str:
    """The text under a `#`/`##`/`###` heading of `markdown`, up to the next heading of the same level."""
    lines = markdown.splitlines()
    for i, line in enumerate(lines):
        m = re.match(r"(#+) (.*)", line)
        if m and m.group(2).strip().startswith(heading):
            level = len(m.group(1))
            body = [line]
            for rest in lines[i + 1 :]:
                n = re.match(r"(#+) ", rest)
                if n and len(n.group(1)) <= level:
                    break
                body.append(rest)
            return "\n".join(body).strip()
    raise SystemExit(f"heading {heading!r} not found")


# --- code map ---------------------------------------------------------------

PUB_ITEM = re.compile(r"^pub (?:const fn|async fn|fn|struct|enum|trait|type|const|static|mod) ([A-Za-z_][A-Za-z0-9_]*)")


def first_sentence(doc: list[str]) -> str:
    text = " ".join(line.strip() for line in doc).strip()
    if not text:
        return ""
    m = re.match(r"(.+?[.!?])(\s|$)", text)
    sentence = m.group(1) if m else text
    return sentence if len(sentence) <= 300 else sentence[:297] + "…"


def module_summary(path: Path) -> tuple[str, list[tuple[str, str, str]]]:
    """Module docs (`//!`) and top-level public items with their first doc sentence."""
    module_doc, items, doc = [], [], []
    in_tests = False
    for raw in read(path).splitlines():
        line = raw.rstrip()
        if line.startswith("//!"):
            module_doc.append(line[3:][1:] if line[3:].startswith(" ") else line[3:])
            continue
        if line.startswith("#[cfg(test)]"):
            in_tests = True
        if in_tests:
            continue
        if line.startswith("///"):
            doc.append(line[3:])
            continue
        if line.startswith("#[") or not line:
            if not line:
                doc = []
            continue
        m = PUB_ITEM.match(line)
        if m:
            kind = line.split()[1] if line.split()[1] != "const" or "fn" not in line.split()[2:3] else "fn"
            if line.startswith("pub mod") and line.endswith(";"):
                doc = []
                continue
            items.append((kind, m.group(1), first_sentence(doc)))
        doc = []
    return "\n".join(module_doc).strip(), items


def code_map() -> str:
    workspace = tomllib.loads(read(ROOT / "Cargo.toml"))
    members = sorted(
        {p for pattern in workspace["workspace"]["members"] for p in ROOT.glob(pattern) if (p / "Cargo.toml").exists()}
    )
    out = ["# Architecture and code map", "", PREAMBLE]
    out.append(
        "AnovaCraft is a Cargo workspace. The statistical engines are GUI-free library crates; `app-model` holds "
        "the application logic (reports, edits, projects) and `app-gui` is a thin egui layer over it. Below, for "
        "each crate: its description, its module documentation, and every top-level public item with the first "
        "sentence of its doc comment. Test modules and GUI test files are summarized by name only.\n"
    )
    out.append("## Workspace layout (from CLAUDE.md)\n")
    out.append(demote(section(read(ROOT / "CLAUDE.md"), "Workspace Layout"), 1).split("\n", 1)[1].strip())
    out.append("")
    for crate in members:
        manifest = tomllib.loads(read(crate / "Cargo.toml"))
        package = manifest.get("package", {})
        name = package.get("name", crate.name)
        out.append(f"## Crate `{name}` (`{crate.relative_to(ROOT)}`)\n")
        if package.get("description"):
            out.append(package["description"] + "\n")
        deps = sorted(manifest.get("dependencies", {}))
        if deps:
            out.append("Dependencies: " + ", ".join(f"`{d}`" for d in deps) + ".\n")
        files = sorted((crate / "src").rglob("*.rs"))
        test_files = [f for f in files if "gui_tests" in f.parts or f.name == "tests.rs" or "tests" in f.parent.name]
        for path in files:
            if path in test_files:
                continue
            rel = path.relative_to(ROOT)
            doc, items = module_summary(path)
            if not doc and not items:
                continue
            out.append(f"### `{rel}`\n")
            if doc:
                out.append(demote(unlink(doc), 3) + "\n")
            if items:
                for kind, ident, sentence in items:
                    out.append(f"- `{kind} {ident}`" + (f": {sentence}" if sentence else ""))
                out.append("")
        if test_files:
            names = ", ".join(f"`{f.relative_to(crate / 'src')}`" for f in test_files)
            out.append(f"Test files: {names}.\n")
        integration = sorted((crate / "tests").glob("*.rs")) if (crate / "tests").exists() else []
        if integration:
            out.append("Integration tests: " + ", ".join(f"`{f.name}`" for f in integration) + ".\n")
    return "\n".join(out)


# --- the files --------------------------------------------------------------


def menu_list(claude: str) -> str:
    """The top-level menus as CLAUDE.md lists them."""
    m = re.search(r"- \*\*Top Menu Bar:\*\*[^\n]*\n((?:  - .*\n)+)", claude)
    if not m:
        raise SystemExit("menu list not found in CLAUDE.md")
    return "\n".join(line[2:] for line in m.group(1).splitlines())


def overview() -> str:
    claude = read(ROOT / "CLAUDE.md")
    paragraphs = unlink(read(ROOT / "docs/manual/src/introduction.md")).split("\n", 1)[1].strip().split("\n\n")
    # The opening paragraph and the one on methods; the rest describes the manual itself.
    intro = "\n\n".join([paragraphs[0]] + [p for p in paragraphs if p.startswith("Every method")])
    glossary = unlink(read(ROOT / "docs/guide/src/glossary.md")).split("\n", 1)[1].strip()
    start = unlink(strip_images(read(ROOT / "docs/guide/src/introduction.md"))).split("\n", 1)[1].strip()
    return "\n".join(
        [
            "# AnovaCraft: overview and glossary",
            "",
            PREAMBLE,
            "## What AnovaCraft is",
            "",
            intro,
            "",
            "## For new users",
            "",
            demote(start, 1),
            "",
            "## Project facts",
            "",
            f"- Source code: {REPO_URL} (open source; see the LICENSE file there).",
            "- Written in Rust: statistical engine crates plus an egui/eframe desktop app for macOS, Windows and "
            "Linux, with a WebAssembly viewer build.",
            "- Automation uses the Rhai scripting language (scripts, formula columns and recorded actions).",
            "- Built clean-room from published statistical literature; its menu names, report titles and visual "
            "design are its own.",
            "- Every statistical routine is tested against an independent reference (NIST StRD, R, SciPy, "
            "published examples); the validation record lists each one.",
            "",
            "## Menus\n",
            menu_list(claude),
            "",
            "## Glossary",
            "",
            demote(glossary, 1),
        ]
    )


def menus_and_features() -> str:
    features = demote(unlink(read(ROOT / "FEATURES.md")), 1).split("\n", 1)[1]
    return "\n".join(
        [
            "# Menus and features",
            "",
            PREAMBLE,
            "This file has two parts: the Quick Guide (every menu item in plain language) and the full feature "
            "list by menu (FEATURES.md).",
            "",
            "## Part 1: Quick Guide",
            "",
            book_text(ROOT / "docs/guide", 2),
            "",
            "## Part 2: Feature list by menu",
            "",
            features.strip(),
        ]
    )


def reference_manual() -> str:
    return "\n".join(
        ["# Reference Manual", "", PREAMBLE, book_text(ROOT / "docs/manual", 1)]
    )


def validation() -> str:
    body = demote(unlink(read(ROOT / "docs/validation.md")), 1).split("\n", 1)[1]
    return "\n".join(["# Validation record", "", PREAMBLE, body.strip()])


def design() -> str:
    claude = read(ROOT / "CLAUDE.md")
    parts = ["# Design and GUI conventions", "", PREAMBLE]
    parts.append(demote(section(claude, "Clean-Room & Legal Provenance Constraints"), 0))
    parts.append(demote(section(claude, "GUI Architecture & Interaction Model"), 0))
    for page in ["reports/reports.md", "reports/plots.md"]:
        path = ROOT / "docs/manual/src" / page
        if path.exists():
            parts.append(demote(unlink(strip_images(read(path))), 1))
    for rel in ["crates/app-gui/src/theme.rs", "crates/app-gui/src/dialog_kit.rs", "crates/app-gui/src/plot_axes.rs"]:
        path = ROOT / rel
        if path.exists():
            doc, _ = module_summary(path)
            if doc:
                parts.append(f"## From `{rel}`\n\n" + demote(doc, 2))
    return "\n\n".join(p.strip() for p in parts)


def development() -> str:
    claude = read(ROOT / "CLAUDE.md")
    parts = ["# Development guide", "", PREAMBLE]
    parts.append(
        "## Building and running\n\n"
        "- Install Rust (stable) from rustup.rs, clone the repository, then `cargo run --release -p app-gui` "
        "starts the desktop app.\n"
        "- `cargo test --workspace --no-fail-fast` runs every test; GUI tests are in `crates/app-gui/src/app/"
        "gui_tests/` (egui_kittest, headless).\n"
        "- `cargo clippy --workspace --all-targets --keep-going -- -D warnings` is the lint gate CI uses.\n"
        "- `anovacraft-run script.rhai` (crate `stat-script`) runs Rhai scripts headless.\n"
        "- `scripts/check-manual.sh` checks both manuals as CI does (needs mdBook for the build step).\n"
        "- Pull requests use `.github/pull_request_template.md` and are merged with a merge commit."
    )
    parts.append("## Project rules (CLAUDE.md)\n\n" + demote(claude, 2))
    for _, path in book_pages(ROOT / "docs/manual"):
        if "maintaining" in path.parts:
            parts.append(demote(unlink(strip_images(read(path))), 1))
    parts.append(demote(unlink(read(ROOT / "ROADMAP.md")), 1))
    pending = ROOT / "docs/pending-ui.md"
    if pending.exists():
        parts.append(demote(read(pending), 1))
    template = ROOT / ".github/pull_request_template.md"
    if template.exists():
        parts.append("## Pull request template\n\n" + demote(read(template), 2))
    workflows = sorted((ROOT / ".github/workflows").glob("*.yml"))
    if workflows:
        lines = ["## Continuous integration", ""]
        for wf in workflows:
            jobs = re.findall(r"^  ([A-Za-z0-9_-]+):\n(?:    .*\n)*?    name: (.+)$", read(wf), re.M)
            lines.append(f"- `{wf.relative_to(ROOT)}`: " + ", ".join(f"{n.strip()} (`{j}`)" for j, n in jobs))
        parts.append("\n".join(lines))
    scripts = []
    for path in sorted((ROOT / "scripts").iterdir()):
        if not path.is_file():
            continue
        header = [l[1:].strip() for l in read(path).splitlines()[1:12] if l.startswith("#") and not l.startswith("#!")]
        doc = " ".join(header).strip()
        if path.suffix == ".py":
            m = re.search(r'"""(.+?)(\n\n|""")', read(path), re.S)
            doc = " ".join(m.group(1).split()) if m else doc
        scripts.append(f"- `scripts/{path.name}`" + (f": {doc}" if doc else ""))
    parts.append("## Scripts\n\n" + "\n".join(scripts))
    return "\n\n".join(p.strip() for p in parts)


def essentials(files: dict[str, str]) -> str:
    """A short pack for assistants that accept little text: overview, glossary, Quick Guide, crate list."""
    guide = book_text(ROOT / "docs/guide", 1)
    layout = demote(section(read(ROOT / "CLAUDE.md"), "Workspace Layout"), 0)
    return "\n\n".join(
        [
            "# AnovaCraft essentials",
            PREAMBLE,
            "This is the short edition of the AnovaCraft knowledge pack and is complete on its own: answer from "
            "it without asking for other files. (A longer edition adds the complete feature list, the Reference "
            "Manual, the validation record, design conventions, a code map and the development guide; use it "
            "only if the user supplies it.)",
            files["01-overview-and-glossary.md"].split("\n", 3)[3].strip(),
            "# Quick Guide",
            guide,
            "# How the code is organised",
            layout,
        ]
    )


COMBINED = "anovacraft-knowledge-pack.md"


def combined(files: dict[str, str], descriptions: dict[str, str]) -> str:
    """Every part in one self-contained file, with a contents list and one preamble."""
    parts = [n for n in files if n != "00-essentials.md"]
    out = [
        "# AnovaCraft knowledge pack (complete, one file)",
        "",
        PREAMBLE,
        "This single file holds the whole pack. Everything an assistant needs is below; there are no other "
        "files to request. Parts:",
        "",
    ]
    for k, name in enumerate(parts, 1):
        title = files[name].split("\n", 1)[0].lstrip("# ").strip()
        out.append(f"{k}. **{title}**: {descriptions[name]} (about {max(1, words(files[name]) // 1000)}k words)")
    for k, name in enumerate(parts, 1):
        title, body = files[name].split("\n", 1)
        body = body.replace(PREAMBLE, "", 1).strip()
        out += ["", "---", "", f"# Part {k}: {title.lstrip('# ').strip()}", "", body]
    return "\n".join(out)


def words(text: str) -> int:
    return len(text.split())


def build() -> dict[str, str]:
    files = {
        "01-overview-and-glossary.md": overview(),
        "02-menus-and-features.md": menus_and_features(),
        "03-reference-manual.md": reference_manual(),
        "04-validation.md": validation(),
        "05-design-and-gui-conventions.md": design(),
        "06-architecture-and-code-map.md": code_map(),
        "07-development-guide.md": development(),
    }
    files = {"00-essentials.md": essentials(files), **files}
    descriptions = {
        "00-essentials.md": "Short version: overview, glossary, every menu item in plain language, crate list. "
        "Use alone when the assistant accepts little text.",
        "01-overview-and-glossary.md": "What AnovaCraft is, project facts, menus and a glossary.",
        "02-menus-and-features.md": "Every menu item in plain language, then the complete feature list by menu.",
        "03-reference-manual.md": "The full Reference Manual: data, reports, plots, every analysis, formulas "
        "and scripting reference.",
        "04-validation.md": "How each statistical routine is verified: reference data, tolerances and tests.",
        "05-design-and-gui-conventions.md": "Clean-room rules, layout, plot navigation, brushing, colours, "
        "dialogs and report conventions.",
        "06-architecture-and-code-map.md": "Crates, modules and public items with their doc lines (no source).",
        "07-development-guide.md": "Building, testing, project rules, roadmap, maintenance, CI and scripts.",
    }
    total = sum(words(t) for t in files.values()) - words(files["00-essentials.md"])
    index = [
        "# AnovaCraft",
        "",
        "> AnovaCraft is an open-source statistics application written in Rust: descriptive statistics, tests, "
        "linear and generalized models, mixed models, predictive models, multivariate methods, design of "
        "experiments, quality and reliability tools, time series and text analysis, with linked interactive "
        "plots and Rhai scripting.",
        "",
        f"Knowledge pack built from {REPO_URL} at commit {git_head()}. Give an assistant all eight files below "
        f"(about {total // 1000}k words without the essentials file), or only `00-essentials.md` "
        f"(about {words(files['00-essentials.md']) // 1000}k words) when it accepts less. Ask it to answer from "
        "these files.",
        "",
        "## Files",
        "",
    ]
    for name in files:
        index.append(f"- [{name}]({name}): {descriptions[name]} (about {max(1, words(files[name]) // 1000)}k words)")
    index += [
        "",
        "## Optional",
        "",
        f"- [Source code]({REPO_URL}): the repository itself, for questions the pack cannot answer.",
    ]
    files[COMBINED] = combined(files, descriptions)
    descriptions[COMBINED] = "All seven parts (01–07) in one self-contained file, for assistants that take one upload."
    index.insert(index.index("## Optional") - 1, f"- [{COMBINED}]({COMBINED}): {descriptions[COMBINED]} "
                 f"(about {words(files[COMBINED]) // 1000}k words)")
    readme = (
        "# AnovaCraft knowledge pack\n\n"
        "Markdown files that let a web chat assistant (Claude, Gemini, ChatGPT, Grok and others) answer "
        "questions about AnovaCraft: how to use it, how it is designed, how the code is organised and how it is "
        "developed.\n\n"
        "**How to use it** (each option is self-contained; the assistant needs nothing else):\n\n"
        f"- **One file, everything:** attach `{COMBINED}` (about 290k words). Best for assistants with large "
        "limits or project knowledge (Gemini, Claude projects, ChatGPT projects).\n"
        "- **One file, short:** attach `00-essentials.md` (about 10k words) for a quick chat or a small limit.\n"
        "- **The parts:** attach `01`–`07` (or only the ones you need), or the zip of this folder made by "
        "`python3 scripts/build-knowledge-pack.py --zip knowledge-pack.zip`. `llms.txt` lists every file.\n\n"
        "**Regenerate** with `python3 scripts/build-knowledge-pack.py` after documentation changes; "
        "`--check` reports whether the folder is current. The files are generated, so edit their sources "
        "(FEATURES.md, the two manuals, docs/validation.md, CLAUDE.md, ROADMAP.md, doc comments) rather than "
        "the pack.\n"
    )
    return {"llms.txt": "\n".join(index) + "\n", "README.md": readme, **{k: v.rstrip() + "\n" for k, v in files.items()}}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="fail if knowledge-pack/ differs from a fresh build")
    parser.add_argument("--zip", metavar="PATH", help="also write the pack as a zip archive at PATH")
    args = parser.parse_args()
    files = build()
    if args.check:
        stale = [n for n, t in files.items() if not (OUT / n).exists() or read(OUT / n) != t]
        # The commit line in llms.txt always differs after a new commit; compare it without that line.
        if "llms.txt" in stale and (OUT / "llms.txt").exists():
            strip = lambda s: re.sub(r"at commit .*?\. Give", "Give", s)
            if strip(read(OUT / "llms.txt")) == strip(files["llms.txt"]):
                stale.remove("llms.txt")
        if stale:
            sys.exit("knowledge-pack/ is out of date: " + ", ".join(stale))
        print("knowledge-pack/ is current")
        return
    OUT.mkdir(exist_ok=True)
    for name, text in files.items():
        (OUT / name).write_text(text, encoding="utf-8")
    for name, text in files.items():
        print(f"{name:40} {words(text):>8} words")
    if args.zip:
        import zipfile

        with zipfile.ZipFile(args.zip, "w", zipfile.ZIP_DEFLATED) as archive:
            for name in files:
                archive.write(OUT / name, f"knowledge-pack/{name}")
        print(f"wrote {args.zip}")


if __name__ == "__main__":
    main()

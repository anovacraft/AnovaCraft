#!/usr/bin/env python3
"""Write THIRD-PARTY-NOTICES.txt: the copyright notices and licence texts of
everything the application contains that AnovaCraft's authors did not write.

    python3 scripts/build-notices.py          # rewrite the file
    python3 scripts/build-notices.py --check  # fail if it is out of date

Part 1 covers the bundled fonts, icons and data (listed in ASSETS below).
Part 2 covers every Rust crate compiled into the shipped programs (the
normal dependencies of `app-gui` and `stat-script`'s `anovacraft-run`, on
macOS, Windows and, for the dashboard web viewer, WebAssembly): each crate's own licence files
from its published package, or the standard text of its licence when the
package ships none. Identical texts are printed once.

The file records a fingerprint of Cargo.lock; the app-gui test
`third_party_notices_match_cargo_lock` fails when Cargo.lock changes without
the file being rebuilt. Needs the crates' sources (any `cargo build` or
`cargo fetch` downloads them).
"""
import json
import os
import pathlib
import re
import subprocess
import textwrap
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD-PARTY-NOTICES.txt"
STANDARD = ROOT / "scripts" / "licences"
# Packages whose binaries ship (the desktop app and anovacraft-run).
BINARIES = {"app-gui", "stat-script"}
TARGETS = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-gnu",  # scripts/build-windows-exe.sh
    "x86_64-pc-windows-msvc",
    "wasm32-unknown-unknown",  # the dashboard web viewer
]
LICENCE_FILE = re.compile(r"(?i)^(licen[cs]e|copying|notice|unlicense|copyright)")
# Preferred choice when a crate offers several licences.
PREFERENCE = ["MIT", "Apache-2.0", "ISC", "BSD-3-Clause", "BSD-2-Clause", "Zlib", "BSL-1.0", "Unlicense", "CC0-1.0"]
# Public-domain dedications: no notice is required.
NO_NOTICE = {"CC0-1.0", "Unlicense", "0BSD"}

FONTS = "crates/app-gui/assets/fonts"
# (title, what it is used for, licence files relative to the repository or,
# for "crate:<name>/...", to that crate's package).
ASSETS = [
    ("Inter typeface", "interface and report text; the AnovaCraft Marks font "
     "(combining bars and hats) is derived from Inter's marks", [f"{FONTS}/Inter-LICENSE.txt"]),
    ("JetBrains Mono typeface", "numbers in tables, monospaced text", [f"{FONTS}/JetBrainsMono-LICENSE.txt"]),
    ("Liberation Sans and Liberation Serif typefaces", "text in exported pictures, serif plot titles",
     [f"{FONTS}/Liberation-LICENSE.txt"]),
    ("DejaVu Sans typeface", "fallback for mathematical and other symbols", [f"{FONTS}/DejaVu-LICENSE.txt"]),
    ("Hack typeface (egui's default monospace font)", "fallback glyphs",
     ["crate:epaint_default_fonts/fonts/Hack-Regular.txt"]),
    ("Ubuntu Light typeface (egui's default font)", "fallback glyphs",
     ["crate:epaint_default_fonts/fonts/UFL.txt"]),
    ("Noto Emoji typeface (egui's default emoji font)", "fallback glyphs",
     ["crate:epaint_default_fonts/fonts/OFL.txt"]),
    ("emoji-icon-font (egui's default symbol font)", "fallback glyphs",
     ["crate:epaint_default_fonts/fonts/emoji-icon-font-mit-license.txt"]),
    ("Lucide icons", "start page and measurement-level icons", ["crates/app-gui/assets/icons/LICENSE-Lucide.txt"]),
    ("VADER sentiment lexicon (C.J. Hutto)", "sentiment scores in text analysis",
     ["crates/stat-text/data/LICENSE-sentiment-lexicon.txt"]),
]

HEADER = """\
AnovaCraft: third-party notices
===============================

AnovaCraft is copyright (c) 2026 The AnovaCraft authors, and is licensed
under either the Apache License, Version 2.0 or the MIT license, at your
option (see COPYRIGHT, LICENSE-APACHE and LICENSE-MIT).

The application also contains the fonts, icons, data and software libraries
below, which their authors license to everyone under the terms reproduced
here. Where a library offers a choice of licences, AnovaCraft uses it under
the first one it offers of {preference}.
Libraries under the Mozilla Public License 2.0 are used unmodified; their
source code is available from https://crates.io/crates/<name>.

The SQLite library compiled into the application is in the public domain.
The Windows build made with MinGW-w64 may also carry libgcc_s_seh-1.dll
(GCC runtime, under the GCC Runtime Library Exception) and
libwinpthread-1.dll (MinGW-w64 winpthreads, MIT-style licence; see
https://www.mingw-w64.org).
The NIST reference datasets in the sample data are works of the United
States government; the other sample datasets cite their published sources in
the Reference Manual (Getting started > Sample data).

Written by scripts/build-notices.py from Cargo.lock (fingerprint {fingerprint});
do not edit by hand.
"""


def fnv1a64(data: bytes) -> str:
    """FNV-1a 64-bit hash, as the app-gui test computes it."""
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"


def lock_fingerprint() -> str:
    return fnv1a64((ROOT / "Cargo.lock").read_bytes().replace(b"\r", b""))


def shipped_packages():
    """Third-party packages compiled into a shipped binary on any release target."""
    found = {}
    for target in TARGETS:
        meta = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
            cwd=ROOT))
        members = set(meta["workspace_members"])
        packages = {p["id"]: p for p in meta["packages"]}
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        stack = [i for i in members if packages[i]["name"] in BINARIES]
        seen = set()
        while stack:
            node = stack.pop()
            if node in seen:
                continue
            seen.add(node)
            for dep in nodes[node]["deps"]:
                if any(k["kind"] is None for k in dep["dep_kinds"]):
                    stack.append(dep["pkg"])
        for node in seen - members:
            found[node] = packages[node]
    return sorted(found.values(), key=lambda p: (p["name"], p["version"]))


def licence_files(package):
    folder = pathlib.Path(package["manifest_path"]).parent
    files = []
    for entry in sorted(folder.iterdir()):
        if not LICENCE_FILE.match(entry.name):
            continue
        if entry.is_dir():
            files += sorted(f for f in entry.iterdir() if f.is_file())
        else:
            files.append(entry)
    if package.get("license_file"):
        extra = folder / package["license_file"]
        if extra.is_file() and extra not in files:
            files.append(extra)
    return files


def chosen_licences(expression):
    """The licences a crate is used under: one choice per OR, every AND part."""
    expression = (expression or "").replace("/", " OR ")
    parts = []
    for conjunct in re.split(r"\s+AND\s+", expression.replace("(", "").replace(")", "")):
        options = [o.strip() for o in re.split(r"\s+OR\s+", conjunct) if o.strip()]
        options = [o.removesuffix("+") for o in options]
        ranked = sorted(options, key=lambda o: PREFERENCE.index(o) if o in PREFERENCE else len(PREFERENCE))
        if ranked:
            parts.append(ranked[0])
    return parts


def authors(package):
    names = [re.sub(r"\s*<[^>]*>", "", a).strip() for a in package.get("authors") or []]
    names = [n for n in names if n]
    return ", ".join(names) if names else f"the {package['name']} developers"


def normalised(text):
    return re.sub(r"\s+", " ", text).strip()


def read(path):
    return path.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip()


def build():
    packages = shipped_packages()
    by_name = {p["name"]: p for p in packages}
    out = [HEADER.format(preference=", ".join(PREFERENCE[:6]), fingerprint=lock_fingerprint())]

    out.append("\nPART 1. FONTS, ICONS AND DATA\n" + "=" * 29 + "\n")
    for title, use, sources in ASSETS:
        used = textwrap.fill(f"Used for: {use}", 72)
        out.append(f"\n{title}\n{used}\n" + "-" * 72 + "\n")
        for source in sources:
            if source.startswith("crate:"):
                name, rel = source[len("crate:"):].split("/", 1)
                path = pathlib.Path(by_name[name]["manifest_path"]).parent / rel
            else:
                path = ROOT / source
            out.append(read(path) + "\n")

    # Licence texts, printed once each, with the crates they apply to.
    texts = {}  # normalised text -> [heading text, [crate labels]]

    def add(text, label):
        key = normalised(text)
        texts.setdefault(key, [text, []])[1].append(label)

    rows = []
    for p in packages:
        label = f"{p['name']} {p['version']}"
        declared = p.get("license") or "see licence file"
        rows.append(f"{p['name']:<34} {p['version']:<24} {declared}")
        files = licence_files(p)
        if files:
            for f in files:
                add(read(f), label)
            continue
        for licence in chosen_licences(p.get("license")):
            if licence in NO_NOTICE:
                continue
            if licence in ("OFL-1.1", "Ubuntu-font-1.0"):
                continue  # egui's fonts: in part 1
            standard = STANDARD / f"{licence}.txt"
            if not standard.is_file():
                sys.exit(f"{label}: no licence file and no standard text for {licence} in {STANDARD}")
            add(f"Copyright (c) {authors(p)}\n\n{read(standard)}", label)

    out.append(f"\n\nPART 2. RUST LIBRARIES ({len(packages)})\n" + "=" * 29 + "\n\n")
    out.append(f"{'Crate':<34} {'Version':<24} Licence\n")
    out.append("\n".join(rows) + "\n")

    out.append("\n\nPART 3. LICENCE TEXTS OF THE RUST LIBRARIES\n" + "=" * 43 + "\n")
    for text, labels in sorted(texts.values(), key=lambda t: (t[1][0], t[0])):
        applies = textwrap.fill("Applies to: " + ", ".join(labels), 72, break_on_hyphens=False)
        out.append("\n" + "-" * 72 + "\n" + applies + "\n\n" + text + "\n")
    return "".join(out)


def main():
    text = build()
    if "--check" in sys.argv:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            sys.exit("THIRD-PARTY-NOTICES.txt is out of date: run python3 scripts/build-notices.py")
        print("THIRD-PARTY-NOTICES.txt is up to date")
        return
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)} ({len(text) // 1024} KB)")


if __name__ == "__main__":
    main()

# AnovaCraft knowledge pack

Markdown files that let a web chat assistant (Claude, Gemini, ChatGPT, Grok and others) answer questions about AnovaCraft: how to use it, how it is designed, how the code is organised and how it is developed.

**How to use it** (each option is self-contained; the assistant needs nothing else):

- **One file, everything:** attach `anovacraft-knowledge-pack.md` (about 290k words). Best for assistants with large limits or project knowledge (Gemini, Claude projects, ChatGPT projects).
- **One file, short:** attach `00-essentials.md` (about 10k words) for a quick chat or a small limit.
- **The parts:** attach `01`–`07` (or only the ones you need), or the zip of this folder made by `python3 scripts/build-knowledge-pack.py --zip knowledge-pack.zip`. `llms.txt` lists every file.

**Regenerate** with `python3 scripts/build-knowledge-pack.py` after documentation changes; `--check` reports whether the folder is current. The files are generated, so edit their sources (FEATURES.md, the two manuals, docs/validation.md, CLAUDE.md, ROADMAP.md, doc comments) rather than the pack.

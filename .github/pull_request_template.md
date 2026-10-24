## What changes for the user

Before:

After:

## How

## Checklist

- [ ] Tests cover each new or changed statistical routine, against an independent reference
- [ ] `docs/validation.md` updated with each new or changed statistical routine, its reference data and its test (or no statistical routine changed)
- [ ] `FEATURES.md` updated for anything a user can see (or nothing user-visible changed)
- [ ] Reference Manual (`docs/manual`) and, for menu items, the Quick Guide (`docs/guide`) updated for anything a user can see, including changes to features already considered done; changed screens re-rendered with `scripts/capture-manual-shots.sh` or marked and listed in `docs/manual/src/maintaining/screenshots.md` (or nothing user-visible changed)
- [ ] Manual PDFs rebuilt with `scripts/build-manual-pdfs.sh` if either manual changed
- [ ] `ROADMAP.md` item moved to `FEATURES.md` if one shipped

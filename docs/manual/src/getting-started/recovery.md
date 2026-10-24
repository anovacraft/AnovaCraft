# Crash reports and recovery

AnovaCraft is built so that a fault in one part does not cost you your work.

## An analysis that fails

If an analysis stops on an internal error (a bug, rather than data it cannot
use), its report says **stopped with an internal error** and shows the
message in place of its sections. The rest of the program keeps running,
and the report keeps its settings, so it is saved with the project as it
was. It computes again whenever its data, row states or settings change.

## Crash reports

Each internal error, and anything that makes the program stop, writes a
short text file: what happened, where in the program, the version, and a
backtrace. The files go to

| System | Folder |
|---|---|
| macOS | `~/Library/Logs/AnovaCraft` |
| Windows | `%LOCALAPPDATA%\AnovaCraft\Logs` |
| Linux | `$XDG_STATE_HOME/anovacraft/logs` (normally `~/.local/state/anovacraft/logs`) |

Files for errors the program survived are named `analysis-error-…`, those
for stops `crash-…`; the newest 20 are kept. **Help ▸ Show crash reports**
opens the folder. The files contain no data from your tables.

## Recovery copies

Every two minutes, each open project with changes since it was opened or
saved is written, on a background thread, as an ordinary project file to
the `Recovery` folder beside the settings file (for example
`~/Library/Application Support/Statistical Engine/Recovery` on macOS). Row
selections and the window layout are left out, as they are when the program
decides whether a project has unsaved changes. Saving a project deletes its
copy, and quitting normally deletes all of them.

When AnovaCraft starts after a run that did not end normally (it crashed,
was forced to quit, or the computer lost power), it shows **Restore unsaved
work**, listing each project kept, the file it came from and when the copy
was written:

- **Restore** opens the copies as projects with unsaved changes. Saving one
  writes to the file it came from; a project that had never been saved asks
  for a name.
- **Discard** deletes the copies.
- **Decide later** keeps them and asks again at the next start.
- **Show crash report** (when the run ended in a crash) opens its report.

A copy reflects the project as it was at most two minutes before the end.
Several windows of the program running at once each keep their own copies
and do not offer each other's.

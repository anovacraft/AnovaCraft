# Copying and pasting

## Selecting what to copy

Drag across cells, Shift-click, or hold Shift with the arrow keys to select
a block of cells; click column headings to select whole columns (see
[Editing the data grid](editing.md#selecting-cells-columns-and-rows)).

## Copy

Edit ▸ Copy (⌘C / Ctrl+C) puts text on the clipboard, tab-separated so it
pastes straight into a spreadsheet:

- the selected columns, every row, with a first line of column names;
- otherwise, a selected block of cells;
- otherwise, when two or more rows are selected, those rows with a first line
  of column names;
- otherwise, the cell under the cursor.

Edit ▸ Cut (⌘X / Ctrl+X) copies the selected columns or block of cells (or
the cell under the cursor) and then clears them. It never takes whole
selected rows; use Delete selected rows for that.

Edit ▸ Copy selected rows with column names copies the selected rows (any
number) with their column names.

## Paste

Edit ▸ Paste (⌘V / Ctrl+V) reads tab- or comma-separated text into the grid,
starting at the cell cursor (or at the top of the first selected column).
Text with a tab is split at tabs. Text without tabs is split at commas only
when every line splits into the same number of fields, not every comma is
followed by a space (so a column of names written `Smith, John` stays one
column), the table has more than one column, and the pieces fit the
columns; otherwise each line is one value. The status line says when the
text was split on commas.

- Rows and columns are added as needed. New columns take their type from the
  pasted values.
- Every value is checked against its column's type. One bad value stops the
  whole paste, and nothing changes.
- A first line that repeats the column names is skipped.
- One Undo removes the whole paste.

File ▸ New table from clipboard makes a new table from the clipboard text
through the [Open Data dialog](open-data.md), which proposes a first line of
names as the column names and skips title lines above the table.

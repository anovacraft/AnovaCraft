# Dates and times

A date column holds points in time (or lengths of time) and has four
displays, chosen from the column's **Date display** menu:

| Display | Example |
|---|---|
| Date | 2026-10-13 |
| Date and time | 2026-10-13 15:57:58 |
| Time of day | 15:57:58 (seconds after midnight) |
| Duration | 1:30:00, or 2 03:15:00 for two days and more |

The same menu sets **Decimal places of seconds** (up to six) and **Show at**
a UTC offset.

## Importing dates

Import recognises ISO 8601 and RFC 3339 text, slash dates (`10/13/2026`),
month-name dates (`13 Oct 2026`) and spreadsheet date cells (in both the 1900
and 1904 serial systems). In `.ods` files, times of day arrive as durations.

Slash dates such as `03/04/2026` are ambiguous. When a column's order cannot
be told apart from its values, it is read month first and the import says so.
If the column was really day first, choose **Swap day and month** from its
Data type menu.

## Converting

A date column converts to and from text, and to and from numbers (seconds
since 1 January 1970, UTC). Text converted to a column shown as **Date**
keeps only the day of each value, as typing does: a time of day in the text
is dropped. In formulas, the date functions extract parts,
take differences, add intervals and format dates (see
[Formula columns](../reference/formulas.md)).

## Dates in analyses and plots

Dates analyse as continuous values. In Describe ▸ Distribution, locations
(mean, quantiles, minimum and maximum) are reported as dates and spreads
(standard deviation, MAD, IQR) as lengths of time. Plot axes for dates tick
on calendar units (seconds, minutes, hours, days, months or years) and are
labelled as dates and times, with no zero baseline.

## Exporting

Text and JSON export write ISO 8601. `.xlsx` export writes date cells with
number formats, so spreadsheets show them as dates.

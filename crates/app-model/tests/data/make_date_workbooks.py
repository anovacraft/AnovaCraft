"""Build the spreadsheet fixtures for the date-import tests in `file_io.rs`.

Writes a flat OpenDocument spreadsheet (`.fods`) whose date, date-time, time
and duration cells carry their ISO 8601 values (`office:date-value`,
`office:time-value`), once with the default 1899-12-30 null date and once with
the 1904-01-01 null date, then has LibreOffice save them as `.xlsx` and `.ods`.
LibreOffice is an independent implementation of the 1900 and 1904 serial
date systems (ECMA-376 Part 1 §18.17.4), so reading its files back checks our
serial conversion end to end. Dates before 1900-03-01 are left out: there
LibreOffice writes its own serials (1900-01-01 as 2) rather than the 1900
system's (1900-01-01 as 1, ECMA-376 and openpyxl agree), so the files would
disagree with the standard rather than test it. Run from this folder:

    python3 make_date_workbooks.py

Needs `soffice` (LibreOffice) on the PATH.
"""

import os
import shutil
import subprocess
import tempfile

# (date, date-time, time of day, duration) per row: ISO 8601 text.
ROWS = [
    ("2026-10-13", "2026-10-13T15:57:58", "PT15H57M58S", "PT1H30M"),
    ("1900-03-01", "1900-03-02T06:00:00", "PT00H00M01S", "PT36H"),
    ("2000-02-29", "2099-12-31T23:59:59", "PT12H", "PT0S"),
    ("1904-01-02", "1970-01-01T00:00:00", "PT23H59M59S", "P2DT3H15M"),
]


def fods(null_date):
    header = "".join(
        f'<table:table-cell office:value-type="string"><text:p>{h}</text:p></table:table-cell>'
        for h in ("date", "stamp", "clock", "took")
    )
    body = ""
    for d, dt, t, dur in ROWS:
        body += (
            "<table:table-row>"
            f'<table:table-cell office:value-type="date" office:date-value="{d}" table:style-name="d"/>'
            f'<table:table-cell office:value-type="date" office:date-value="{dt}" table:style-name="dt"/>'
            f'<table:table-cell office:value-type="time" office:time-value="{t}" table:style-name="t"/>'
            f'<table:table-cell office:value-type="time" office:time-value="{dur}" table:style-name="dur"/>'
            "</table:table-row>"
        )
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
  xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0"
  office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.spreadsheet">
 <office:automatic-styles>
  <number:date-style style:name="Nd"><number:year number:style="long"/><number:text>-</number:text><number:month number:style="long"/><number:text>-</number:text><number:day number:style="long"/></number:date-style>
  <number:date-style style:name="Ndt"><number:year number:style="long"/><number:text>-</number:text><number:month number:style="long"/><number:text>-</number:text><number:day number:style="long"/><number:text> </number:text><number:hours number:style="long"/><number:text>:</number:text><number:minutes number:style="long"/><number:text>:</number:text><number:seconds number:style="long"/></number:date-style>
  <number:time-style style:name="Nt"><number:hours number:style="long"/><number:text>:</number:text><number:minutes number:style="long"/><number:text>:</number:text><number:seconds number:style="long"/></number:time-style>
  <number:time-style style:name="Ndur" number:truncate-on-overflow="false"><number:hours/><number:text>:</number:text><number:minutes number:style="long"/><number:text>:</number:text><number:seconds number:style="long"/></number:time-style>
  <style:style style:name="d" style:family="table-cell" style:data-style-name="Nd"/>
  <style:style style:name="dt" style:family="table-cell" style:data-style-name="Ndt"/>
  <style:style style:name="t" style:family="table-cell" style:data-style-name="Nt"/>
  <style:style style:name="dur" style:family="table-cell" style:data-style-name="Ndur"/>
 </office:automatic-styles>
 <office:body><office:spreadsheet>
  <table:calculation-settings><table:null-date table:date-value="{null_date}"/></table:calculation-settings>
  <table:table table:name="dates"><table:table-row>{header}</table:table-row>{body}</table:table>
 </office:spreadsheet></office:body>
</office:document>
"""


def convert(src, fmt, out_name):
    with tempfile.TemporaryDirectory() as out, tempfile.TemporaryDirectory() as profile:
        subprocess.run(
            ["soffice", f"-env:UserInstallation=file://{profile}", "--headless", "--convert-to", fmt,
             "--outdir", out, src],
            check=True, capture_output=True,
        )
        produced = os.path.join(out, os.path.splitext(os.path.basename(src))[0] + "." + fmt)
        shutil.copy(produced, out_name)


for null_date, stem in (("1899-12-30", "dates_1900"), ("1904-01-01", "dates_1904")):
    with open(f"{stem}.fods", "w", encoding="utf-8") as f:
        f.write(fods(null_date))
    convert(f"{stem}.fods", "xlsx", f"{stem}.xlsx")
convert("dates_1900.fods", "ods", "dates_1900.ods")

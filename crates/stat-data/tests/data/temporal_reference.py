"""Reference values for the date and time tests in `stat-data`.

Computed with Python's standard `datetime` module and `openpyxl`, two
independent open-source implementations of the proleptic Gregorian calendar,
ISO 8601 text and spreadsheet serial dates. Regenerate with

    python3 temporal_reference.py > temporal_reference.csv

Every row is `kind,text,expected` where `expected` is the value in whole
microseconds (seconds since 1970-01-01T00:00:00 UTC, seconds after midnight,
or a length of time), so the comparison involves no floating-point text.
Kinds:

* `date`      — `YYYY-MM-DD` from `date.isoformat()`.
* `instant`   — RFC 3339 text from `datetime.isoformat()` with a UTC offset.
* `named`     — `13 Oct 2026 15:57:58` from `strftime("%d %b %Y %H:%M:%S")`, UTC.
* `serial1900` / `serial1904` — a spreadsheet serial (`repr` of the float) and
  `openpyxl.utils.datetime.from_excel` in that date system, read as UTC.
"""

import random
from datetime import date, datetime, timedelta, timezone

from openpyxl.utils.datetime import CALENDAR_MAC_1904, CALENDAR_WINDOWS_1900, from_excel

EPOCH = datetime(1970, 1, 1, tzinfo=timezone.utc)
rng = random.Random(20261013)


def micros(dt):
    d = dt - EPOCH
    return (d.days * 86400 + d.seconds) * 1_000_000 + d.microseconds


def random_date():
    return date.fromordinal(rng.randint(date(1, 1, 1).toordinal(), date(9999, 12, 31).toordinal()))


rows = []
fixed = [date(1900, 2, 28), date(1900, 3, 1), date(2000, 2, 29), date(2100, 2, 28), date(2100, 3, 1),
         date(1970, 1, 1), date(1969, 12, 31), date(1, 1, 1), date(9999, 12, 31), date(1582, 10, 15)]
for d in fixed + [random_date() for _ in range(20_000)]:
    rows.append(("date", d.isoformat(), micros(datetime(d.year, d.month, d.day, tzinfo=timezone.utc))))

for _ in range(5_000):
    d = random_date()
    if d.year < 2:
        continue
    offset = timedelta(minutes=rng.randint(-12 * 4, 14 * 4) * 15)
    digits = rng.choice([0, 0, 3, 6])
    micro = rng.randrange(1_000_000) // 10 ** (6 - digits) * 10 ** (6 - digits) if digits else 0
    dt = datetime(d.year, d.month, d.day, rng.randrange(24), rng.randrange(60), rng.randrange(60), micro,
                  tzinfo=timezone(offset))
    rows.append(("instant", dt.isoformat(), micros(dt)))

for _ in range(1_000):
    d = random_date()
    dt = datetime(d.year, d.month, d.day, rng.randrange(24), rng.randrange(60), rng.randrange(60),
                  tzinfo=timezone.utc)
    rows.append(("named", dt.strftime("%d %b %Y %H:%M:%S").lstrip("0") if d.year >= 1000 else None, micros(dt)))

for system, calendar, low, high in (("serial1900", CALENDAR_WINDOWS_1900, 1, 2_958_465),
                                   ("serial1904", CALENDAR_MAC_1904, -1000, 2_957_002)):
    serials = [1.0, 59.0, 61.0, 25569.0, 46308.5, 2958465.0] if system == "serial1900" else [-1000.0, 1.0, 1462.0, 45000.25]
    serials += [rng.randint(low, high) + rng.randrange(86_400) / 86_400 for _ in range(2_000)]
    for s in serials:
        if system == "serial1900" and 60 <= s < 61:
            continue  # 29 February 1900 does not exist
        dt = from_excel(s, calendar).replace(tzinfo=timezone.utc)
        rows.append((system, repr(s), micros(dt)))

print("kind,text,expected")
for kind, text, expected in rows:
    if text is not None:
        print(f"{kind},{text},{expected}")

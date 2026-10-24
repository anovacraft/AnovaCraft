"""Independent reference results for stat-data's table operations, computed
with pandas and numpy. Run from this directory to regenerate
reference_cases.json:

    python3 reference_cases.py

Each case holds its input tables, the operation's options, and the expected
table. Tables are lists of columns {name, type, values}: type "number"
(null = missing), "text" ("" = missing) or "date" (seconds since 1970,
null = missing).

Where pandas' defaults differ from the documented behaviour of stat-data, the
script adapts pandas, never the other way round:
* merge: pandas matches missing keys to each other; SQL (and stat-data) never
  does, so missing keys are replaced by unique placeholders before merging and
  restored afterwards. pandas orders right and outer joins by key; the result
  is put in stat-data's documented order (left rows in order, each with its
  matches in right-table order, then unmatched right rows) using row ids.
* melt lists all rows of the first stacked column, then the second; stat-data
  lists each source row's stacked values together, so rows are re-sorted by
  (source row, stacked column).
* pivot sorts IDs; stat-data keeps IDs in order of first appearance, so rows
  are reindexed by first appearance.
"""
import json
import math

import numpy as np
import pandas as pd

rng = np.random.default_rng(20261013)


def enc(df):
    cols = []
    for name in df.columns:
        s = df[name]
        if pd.api.types.is_datetime64_any_dtype(s):
            vals = [None if pd.isna(v) else (v - pd.Timestamp(0)).total_seconds() for v in s]
            cols.append(dict(name=str(name), type="date", values=vals))
        elif pd.api.types.is_timedelta64_dtype(s):
            vals = [None if pd.isna(v) else v.total_seconds() for v in s]
            cols.append(dict(name=str(name), type="date", values=vals))
        elif pd.api.types.is_numeric_dtype(s):
            vals = [None if (v is None or (isinstance(v, float) and math.isnan(v))) else float(v) for v in s]
            cols.append(dict(name=str(name), type="number", values=vals))
        else:
            vals = ["" if (v is None or (isinstance(v, float) and math.isnan(v))) else str(v) for v in s]
            cols.append(dict(name=str(name), type="text", values=vals))
    return cols


def text(values):
    return pd.Series([np.nan if v == "" else v for v in values], dtype=object)


def dates(days):
    return pd.to_datetime(pd.Series([np.nan if d is None else d * 86400.0 for d in days]), unit="s")


cases = []

# ------------------------------------------------------------------ data ---
n = 40
groups = ["north", "south", "east", ""]
base = pd.DataFrame({
    "region": text([groups[i] for i in rng.integers(0, 4, n)]),
    "shift": text([["day", "night"][i] for i in rng.integers(0, 2, n)]),
    "yield": np.where(rng.random(n) < 0.1, np.nan, np.round(rng.normal(50, 8, n), 3)),
    "count": rng.integers(0, 5, n).astype(float),
    "made": dates([None if rng.random() < 0.1 else int(d) for d in rng.integers(19000, 19060, n)]),
})

# ------------------------------------------------------------------ sort ---
for keys, asc in [(["region", "yield"], [True, False]), (["made"], [True]), (["count", "shift"], [False, True])]:
    out = base.sort_values(keys, ascending=asc, na_position="last", kind="stable")
    cases.append(dict(op="sort", inputs=[enc(base)], keys=keys, descending=[not a for a in asc], expected=enc(out)))

# ------------------------------------------------------------ duplicates ---
for subset in [["region", "shift"], ["count"], ["region", "made"]]:
    out = base.drop_duplicates(subset=subset, keep="first")
    cases.append(dict(op="duplicates", inputs=[enc(base)], columns=subset, expected=enc(out)))

# --------------------------------------------------------------- combine ---
a = base.iloc[:7][["region", "yield", "made"]]
b = pd.DataFrame({"yield": [1.5, np.nan, 2.25], "batch": text(["b1", "", "b3"]), "region": text(["west", "", "north"])})
out = pd.concat([a.assign(Source="first"), b.assign(Source="second")], ignore_index=True, sort=False)
out = out[["region", "yield", "made", "batch", "Source"]]
cases.append(dict(op="combine", inputs=[enc(a), enc(b)], names=["first", "second"], source="Source", expected=enc(out)))

# ------------------------------------------------------------------ join ---
left = pd.DataFrame({
    "region": text([groups[i] for i in rng.integers(0, 4, 25)]),
    "shift": text([["day", "night", ""][i] for i in rng.integers(0, 3, 25)]),
    "amount": np.round(rng.normal(10, 3, 25), 2),
})
right = pd.DataFrame({
    "shift": text([["day", "night", "", "swing"][i] for i in rng.integers(0, 4, 12)]),
    "region": text([groups[i] for i in rng.integers(0, 4, 12)]),
    "rate": np.round(rng.uniform(0, 1, 12), 3),
    "amount": np.round(rng.normal(0, 1, 12), 2),
})


def sql_merge(l, r, on, how, right_name):
    l = l.copy()
    r = r.copy()
    l["_l"] = np.arange(len(l), dtype=float)
    r["_r"] = np.arange(len(r), dtype=float)
    # Missing keys must never match: give each one a unique placeholder.
    for k in on:
        l[k] = pd.Series([f"~missing L{i}~" if pd.isna(v) else str(v) for i, v in enumerate(l[k])], dtype=object)
        r[k] = pd.Series([f"~missing R{i}~" if pd.isna(v) else str(v) for i, v in enumerate(r[k])], dtype=object)
    m = l.merge(r, on=on, how=how, suffixes=("", f" ({right_name})"), sort=False)
    for k in on:
        m[k] = pd.Series([np.nan if isinstance(v, str) and v.startswith("~missing ") else v for v in m[k]], index=m.index, dtype=object)
    m = m.sort_values(["_l", "_r"], na_position="last", kind="stable")
    return m.drop(columns=["_l", "_r"])


for how in ["inner", "left", "right", "outer"]:
    out = sql_merge(left, right, ["region", "shift"], how, "rates")
    cases.append(dict(op="join", inputs=[enc(left), enc(right)], names=["orders", "rates"], keys=[["region", "region"], ["shift", "shift"]],
                      how=how, expected=enc(out)))

# ----------------------------------------------------------------- stack ---
wide = pd.DataFrame({
    "id": text([f"s{i}" for i in range(8)]),
    "site": text(["a", "b"] * 4),
    "before": np.where(rng.random(8) < 0.2, np.nan, np.round(rng.normal(5, 1, 8), 2)),
    "after": np.round(rng.normal(6, 1, 8), 2),
    "follow": np.round(rng.normal(7, 1, 8), 2),
})
stacked_cols = ["before", "after", "follow"]
long = wide.reset_index().melt(id_vars=["index", "id", "site"], value_vars=stacked_cols, var_name="time", value_name="score")
long["_k"] = long["time"].map(stacked_cols.index)
long = long.sort_values(["index", "_k"], kind="stable").drop(columns=["index", "_k"])
cases.append(dict(op="stack", inputs=[enc(wide)], columns=stacked_cols, label="time", value="score", expected=enc(long)))

# --------------------------------------------------------------- unstack ---
obs = pd.DataFrame({
    "subject": text(["p3", "p3", "p1", "p2", "p1", "p2", "p3", "p1"]),
    "visit": text(["v1", "v2", "v1", "v2", "v2", "v1", "v3", ""]),
    "bp": [120.0, 118.0, 131.0, 125.0, np.nan, 122.0, 117.0, 140.0],
    "hr": [70.0, 72.0, 80.0, 75.0, 78.0, 74.0, 69.0, 90.0],
})
kept = obs[obs["visit"].notna()]
order = list(pd.unique(kept["subject"]))
for values in [["bp"], ["bp", "hr"]]:
    p = kept.pivot(index="subject", columns="visit", values=values).reindex(order)
    p.columns = [lvl if len(values) == 1 else f"{v} {lvl}" for v, lvl in p.columns]
    p = p.reset_index()
    cases.append(dict(op="unstack", inputs=[enc(obs)], label="visit", values=values, ids=["subject"], expected=enc(p)))

# ------------------------------------------------------------- transpose ---
small = pd.DataFrame({"name": text(["r1", "r2", "r3"]), "x": [1.0, np.nan, 3.5], "y": [4.0, 5.0, 6.25], "z": [0.0, -1.0, 2.0]})
t = small.set_index("name")[["x", "y", "z"]].T.reset_index().rename(columns={"index": "Column"})
t.columns.name = None
cases.append(dict(op="transpose", inputs=[enc(small)], columns=["x", "y", "z"], label="name", expected=enc(t)))

# ------------------------------------------------------------- summarize ---
H_F = {7: "linear", 8: "median_unbiased", 6: "weibull", 2: "averaged_inverted_cdf"}


def summarize(df, by, cols, qtype):
    g = df.groupby(by, dropna=False, sort=True) if by else None
    parts = [(key if isinstance(key, tuple) else (key,), sub) for key, sub in g] if by else [((), df)]
    # Groups in sort order with missing values last (pandas already does this).
    rows = []
    for key, sub in parts:
        row = dict(zip(by, key))
        row["Rows"] = float(len(sub))
        for c in cols:
            s = sub[c]
            is_date = pd.api.types.is_datetime64_any_dtype(s)
            x = s.dropna()
            v = ((x - pd.Timestamp(0)).dt.total_seconds() if is_date else x).to_numpy(dtype=float)
            row[f"{c} N"] = float(len(v))
            row[f"{c} missing"] = float(s.isna().sum())
            row[f"{c} mean"] = v.mean() if len(v) else np.nan
            row[f"{c} SD"] = v.std(ddof=1) if len(v) > 1 else np.nan
            row[f"{c} min"] = v.min() if len(v) else np.nan
            row[f"{c} max"] = v.max() if len(v) else np.nan
            row[f"{c} median"] = np.median(v) if len(v) else np.nan
            for q in (0.1, 0.25, 0.9):
                row[f"{c} {q * 100:g}% quantile"] = np.quantile(v, q, method=H_F[qtype]) if len(v) else np.nan
            if not is_date:
                row[f"{c} sum"] = v.sum()
        rows.append(row)
    out = pd.DataFrame(rows)
    for c in by:
        if pd.api.types.is_datetime64_any_dtype(df[c]):
            out[c] = pd.to_datetime(out[c])
    for c in cols:
        if pd.api.types.is_datetime64_any_dtype(df[c]):
            for s in ["mean", "min", "max", "median", "10% quantile", "25% quantile", "90% quantile"]:
                out[f"{c} {s}"] = pd.to_datetime(out[f"{c} {s}"], unit="s")
            out[f"{c} SD"] = pd.to_timedelta(out[f"{c} SD"], unit="s")
    return out


for by, cols, qtype in [(["region"], ["yield", "count"], 8), (["region", "shift"], ["yield"], 7), ([], ["yield", "made"], 6), (["shift"], ["made"], 2)]:
    cases.append(dict(op="summarize", inputs=[enc(base)], groups=by, columns=cols, quantile_type=qtype, expected=enc(summarize(base, by, cols, qtype))))

with open("reference_cases.json", "w") as f:
    json.dump(dict(generator=f"pandas {pd.__version__}, numpy {np.__version__}", cases=cases), f, indent=1)
print(len(cases), "cases")

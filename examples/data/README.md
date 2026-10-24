# Example data files

Files for trying the import, export and project features. All but the
Michelson, leukemia and sleep data are **simulated** (see
`crates/app-model/src/sample.rs` for the models used). Michelson is from the
NIST Statistical Reference Datasets, the leukemia remission times are the
published data of Freireich et al. (1963), and the sleep data are Student's
(1908).
Regenerate the text files with
`cargo run -p app-model --example export_samples -- examples/data`.

Plain data files (CSV, TSV, JSON, Excel) store values only, so a number
column always opens as continuous. Data that need nominal or ordinal number
columns are therefore saved as projects, which keep each column's
measurement level; every file below opens ready to analyse without changing
any column type.

| File | Format feature | Suggested analyses |
|---|---|---|
| `plant_growth.acraft` | project, data only (Greenhouse is nominal) | Model ▸ Linear model: Height ~ Fertilizer + Light hours + Greenhouse |
| `customer_survey.acraft` | project, data only (Satisfaction is ordinal) | Bivariate Region × Channel; Distribution of Satisfaction |
| `clinic_wait_times.tsv` | tab-separated, missing values | Describe ▸ Distribution of Wait; Bivariate Wait by Clinic |
| `catalyst_experiment.json` | JSON rows | Linear model Yield ~ Temperature + Pressure + Catalyst; Response Explorer |
| `fill_weights_semicolon.txt` | semicolon-delimited `.txt` (delimiter detected) | Quality ▸ Control chart, X̄ and R, subgroup 5 |
| `michelson_speed_of_light.csv` | comma-separated | Distribution with normal fit and normality tests |
| `shift_weights_report.xlsx` | Excel laid out like a printed report: a title, a heading line, the table from C4 and a footnote, on two sheets (`make_shift_weights_report.py`) | File ▸ Open shows the Open Data dialog with the table found at C4; tick both sheets and stack them, then Compare ▸ Two or more groups (Weight by Sheet) |
| `two_sheet_workbook.xlsx` | Excel: Catalyst and Clinic wait times sheets, plus an empty sheet | opens as two tabs (the empty sheet is skipped) |
| `plant_growth_outliers_excluded.acraft` | project with reports | reopens with two reports, rows 17 and 44 excluded and selected |
| `bearing_life_test.csv` | comma-separated, censored lifetimes | Reliability ▸ Survival: Hours, status Suspended (non-zero = censored), by Supplier |
| `leukemia_remission.acraft` | project with a survival report | reopens with Kaplan–Meier curves by treatment, log-rank test and Weibull fits |
| `sleep_drug_trial.csv` | stacked: all values in one column, with Drug and Patient columns | Compare ▸ Paired samples (stacked: Extra sleep, condition Drug, subject Patient); Compare ▸ Two or more groups (Extra sleep by Drug) |

// Fit a linear model, exclude the rows its influence diagnostics flag as
// outliers (|studentized residual| > 3), refit, and save both reports.
//
//   anovacraft-run examples/scripts/influential_rows.rhai [OUTPUT_DIRECTORY]
//
// The plant growth data are simulated with two outlying plants (17 and 44).

let out = if args.len() > 0 { args[0] } else { "." };

let t = open_project("../data/plant_growth.acraft");
let model = #{
    response: "Height",
    predictors: ["Fertilizer", "Light hours", "Greenhouse"],
    options: #{ show_diagnostics: true },
};

let first = t.analyze("linear_model", model);
print(`${first.title}`);
print(`R² with every row: ${first.value("Fit Statistics", "R²")}`);

// Influence Diagnostics is a table of flagged rows (or a note when none are).
let flagged = [];
let diagnostics = first.section("Influence Diagnostics");
if type_of(diagnostics) == "array" {
    for row in diagnostics {
        if row["Studentized residual"].abs() > 3.0 {
            flagged.push(row.Row - 1);   // the report numbers rows from 1
        }
    }
}
print(`Excluding rows ${flagged.map(|r| r + 1)}`);
t.exclude_rows(flagged);

let refit = t.analyze("linear_model", model);
print(`R² without them: ${refit.value("Fit Statistics", "R²")}`);
print(`Root mean square error: ${first.value("Fit Statistics", "Root mean square error")} → ${refit.value("Fit Statistics", "Root mean square error")}`);

first.save(`${out}/all_rows.md`);
refit.save(`${out}/outliers_excluded.md`);
refit.save_section("Parameter Estimates", `${out}/estimates.csv`);
t.save_project(`${out}/plant_growth_refit.acraft`);

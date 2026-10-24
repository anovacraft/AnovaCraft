// Fit a model and take its results out of the application: residuals back
// into the table, plots as figures, the table as a workbook.
//
// Runs in the Script window or headless:
//   anovacraft-run examples/scripts/model_outputs.rhai [output folder]
// Files go to the output folder (default: next to this script).

let out = if args.len() > 0 { args[0] + "/" } else { "" };

let t = open_sample("Norris");
let model = t.analyze("linear_model", #{ response: "Reading", predictors: ["Reference"],
                                       options: #{ show_residual_plot: true } });
print(model.title);
print(`R² = ${model.value("Fit Statistics", "R²")}`);

// Predicted values and residuals as new columns of the table.
print(`The model offers: ${model.saveable_columns().map(|c| c.key)}`);
let added = model.save_columns(["predicted", "residual"]);
print(`Added ${added}`);

// The fitted line and the residual plot as vector figures.
model.save_section("Fitted Line", out + "norris_fit.svg");
model.save_section("Residuals vs Predicted", out + "norris_residuals.pdf");

// The table with its new columns, as a workbook and as a project that
// reopens with the model report.
t.save(out + "norris_with_residuals.xlsx");
t.save_project(out + "norris_model.acraft");

// Read the workbook back to check it.
let back = open_table(out + "norris_with_residuals.xlsx");
print(`${back.name}: ${back.rows()} rows, columns ${back.columns()}`);
t.show();

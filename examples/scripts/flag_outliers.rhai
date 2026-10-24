// Flag unusual values in the table that is open, for the Script window.
//
// Open a table first (for example File ▸ Open sample data ▸ Michelson light
// speed), then open this script in File ▸ Script window and press Run. The
// script works on the last continuous column of the current table (or the
// column named in COLUMN):
//
//   1. adds a column of z-scores, (x − mean) / sd;
//   2. selects the rows more than LIMIT standard deviations from the mean, so
//      they light up in the data grid and every plot;
//   3. runs a distribution report on the column;
//   4. opens a summary table (n, mean, sd, median) as a new tab.
//
// The data changes are one step: Edit ▸ Undo removes the z-score column.

const COLUMN = "";   // "" means the last continuous column
const LIMIT = 2.0;

let t = current_table();
let name = COLUMN;
if name == "" {
    // A function that uses `t` needs its own statement: `t.columns().filter(…)`
    // would hold `t` for the whole chain (Rhai reports a "data race").
    let columns = t.columns();
    let continuous = columns.filter(|c| t.level(c) == "continuous");
    if continuous.is_empty() {
        throw `“${t.name}” has no continuous column`;
    }
    name = continuous[continuous.len() - 1];
}

// Mean and standard deviation of the non-missing values.
let values = t.column(name).filter(|x| x != ());
if values.len() < 2 {
    throw `“${name}” needs at least two values`;
}
let mean = values.reduce(|sum, x| sum + x, 0.0) / values.len();
let ss = values.reduce(|sum, x| sum + (x - mean) ** 2, 0.0);
let sd = sqrt(ss / (values.len() - 1));
print(`${name}: n = ${values.len()}, mean = ${mean}, sd = ${sd}`);

let z_name = `z of ${name}`;
if !t.has_column(z_name) {
    t.add_column(z_name, t.column(name).map(|x| if x == () { () } else { (x - mean) / sd }));
}
let flagged = t.select(|row| row[z_name] != () && abs(row[z_name]) > LIMIT);
print(`${flagged} row(s) are more than ${LIMIT} sd from the mean; they are now selected.`);

t.analyze("distribution", #{ column: name });

let summary = t.summarize(#{
    columns: [name],
    statistics: ["n", "mean", "sd", "median"],
    name: `${name} summary`,
});
summary.show();

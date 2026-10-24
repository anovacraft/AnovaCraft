// Summarize several data files the same way: one distribution report per
// numeric column, and a table of means and standard deviations by file.
//
//   anovacraft-run examples/scripts/batch_summaries.rhai [OUTPUT_DIRECTORY]

let out = if args.len() > 0 { args[0] } else { "." };
let files = ["../data/michelson_speed_of_light.csv", "../data/clinic_wait_times.tsv"];

let summary = new_table("Summaries");
let names = [];
let columns = [];
let means = [];
let sds = [];

for file in files {
    let t = open_table(file);
    for column in t.columns() {
        if t.level(column) != "continuous" { continue; }
        let d = t.analyze("distribution", #{ column: column, quantile_type: 8 });
        names.push(t.name);
        columns.push(column);
        means.push(d.value("Summary Statistics", "Mean"));
        sds.push(d.value("Summary Statistics", "Standard deviation"));
        let file_name = `${t.name} - ${column}.md`;
        file_name.replace("/", "-");          // e.g. "Speed (1000 km/s)"
        d.save(`${out}/${file_name}`);
    }
}

summary.add_column("File", names);
summary.add_column("Column", columns);
summary.add_column("Mean", means);
summary.add_column("Std Dev", sds);
summary.save(`${out}/summaries.csv`);
print(`${summary.rows()} columns summarized`);

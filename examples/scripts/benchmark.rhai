// Time the main kinds of script work, to check the engine's speed on your
// machine. Each line gives the step and its time in milliseconds.
//
//   anovacraft-run examples/scripts/benchmark.rhai [ROWS]
//
// Use a release build (cargo build --release -p stat-script); a debug build
// is many times slower. The default is 100 000 rows. Doubling ROWS should
// roughly double every time; a step that quadruples does not scale.

let n = if args.len() > 0 { parse_int(args[0]) } else { 100_000 };

fn report(step, start) {
    print(`${step}: ${(start.elapsed * 1000.0).to_int()} ms`);
}

let start = timestamp();
let s = 0.0;
for i in 0..n { s += i * 0.5; }
report(`script arithmetic (${n} loop steps)`, start);

let start = timestamp();
let x = [];
let g = [];
for i in 0..n {
    x.push((i * 7919) % 1000 / 10.0);
    g.push(`group ${i % 5}`);
}
report(`build two ${n}-value arrays`, start);

let start = timestamp();
let t = new_table("Benchmark");
t.add_column("x", x);
t.add_column("group", g);
t.add_column("y", x.map(|v| 2.0 * v + 1.0));
report("make a table of three columns", start);

let start = timestamp();
let picked = t.select(|row| row.x > 50.0 && row.group != "group 0");
report(`select rows with a test function (${picked} selected)`, start);

let start = timestamp();
let total = 0.0;
for v in t.column("y") { total += v; }
report("read a column and sum it", start);

let start = timestamp();
t.sort(["group", #{ column: "x", descending: true }]);
report("sort by two keys", start);

let start = timestamp();
let s = t.summarize(#{ groups: ["group"], columns: ["x", "y"], statistics: ["n", "mean", "sd", "median"] });
report(`summarize by group (${s.rows()} groups)`, start);

let start = timestamp();
let r = t.analyze("linear_model", #{ response: "y", predictors: ["x", "group"] });
report("fit a linear model", start);

let start = timestamp();
let d = t.analyze("distribution", #{ column: "x", quantile_type: 8 });
report("distribution report", start);

let start = timestamp();
let small = open_sample("Norris");
for i in 0..100 { small.analyze("linear_model", #{ response: "Reading", predictors: ["Reference"] }); }
report("100 small model fits", start);

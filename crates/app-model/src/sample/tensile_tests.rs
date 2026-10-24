//! A sample project for reusing reports and plot styles: three batches of
//! invented tensile tests in tables of the same layout, the first with a
//! formatted graph and a linear model, the last with its columns named
//! differently.

use super::{col, round_to, table};
use crate::LinearModelSpec;
use crate::ReportOptions;
use crate::project::{ProjectFile, ProjectTable, SavedReport, TableId};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel, RowStateManager};
use stat_viz::axis_settings::{GridLines, MajorTicks, MinorTicks};
use stat_viz::{GraphSpec, Role};

/// **Invented** tensile tests of one batch: four specimens, each pulled to
/// 2 % strain in steps of 0.1 %. Stress (MPa) follows an elastic line of
/// slope `modulus` (MPa per %) up to the yield stress `yield_mpa`, then a
/// power-law hardening `yield + 180 × (plastic strain)^0.45`, plus
/// N(0, 3²) noise. Drawn with the workspace's integer-based generator, so
/// it is the same on every platform. `names` are the specimen, strain and
/// stress column names.
fn batch(name: &str, seed: u64, modulus: f64, yield_mpa: f64, names: [&str; 3]) -> DataTable {
    let mut rng = Rng::new(seed);
    let (mut specimen, mut strain, mut stress) = (Vec::new(), Vec::new(), Vec::new());
    for s in 1..=4 {
        // Each specimen yields a little higher or lower.
        let y = yield_mpa + 6.0 * rng.normal();
        for k in 0..=20 {
            let e = f64::from(k) * 0.1;
            let elastic_end = y / modulus;
            let sigma = if e <= elastic_end { modulus * e } else { y + 180.0 * (e - elastic_end).powf(0.45) };
            specimen.push(format!("{}{s}", &name[name.len() - 1..]));
            strain.push(round_to(e, 2));
            stress.push(round_to(sigma + 3.0 * rng.normal(), 1));
        }
    }
    let mut specimen = Column::text(names[0], specimen);
    specimen.level = MeasurementLevel::Nominal;
    table(name, vec![specimen, Column::numeric(names[1], strain), Column::numeric(names[2], stress)])
}

/// Batch A: modulus 2000 MPa per %, yield 250 MPa (seed 811).
#[must_use]
pub fn tensile_batch_a() -> DataTable {
    batch("Tensile batch A", 811, 2000.0, 250.0, ["Specimen", "Strain (%)", "Stress (MPa)"])
}

/// The graph of batch A: stress against strain by specimen with a smoother,
/// formatted (minor grid, ticks every 0.5 % and 100 MPa, serif font,
/// stress from 0 to 500 MPa).
fn stress_strain_graph(t: &DataTable) -> SavedReport {
    let mut spec = GraphSpec::default();
    spec.encoding.assign(Role::X, &t.columns()[1].name);
    spec.encoding.assign(Role::Y, &t.columns()[2].name);
    spec.encoding.assign(Role::Color, &t.columns()[0].name);
    spec.line = Some(stat_viz::LineElement::default());
    let mut style = crate::plot_style::PlotStyle::default();
    style.axes.x.major = MajorTicks::Every(0.5);
    style.axes.y.major = MajorTicks::Every(100.0);
    style.axes.y.minor = MinorTicks::Subdivisions(2);
    style.axes.y.grid = GridLines::MajorAndMinor;
    style.axes.y.range = Some([0.0, 500.0]);
    style.export.font = stat_viz::plot_style::PlotFont::Serif;
    spec.axes.style = Some(style);
    SavedReport::Graph { spec: Box::new(spec) }
}

fn project_table(id: &str, table: DataTable, reports: Vec<SavedReport>) -> ProjectTable {
    let rows = RowStateManager::new(table.n_rows());
    ProjectTable { id: TableId::from(id), table, rows, reports, external: None, lineage: None }
}

/// The tensile tests project: batches A and B with columns Specimen,
/// Strain (%) and Stress (MPa), and batch C with Test piece, Elongation (%)
/// and stress (MPa). Batch A holds a formatted stress–strain graph and a
/// linear model of stress on strain; B and C have no reports, ready for
/// Duplicate on another table, Paste plot style or a template.
#[must_use]
pub fn tensile_project() -> ProjectFile {
    let a = tensile_batch_a();
    let b = batch("Tensile batch B", 812, 2050.0, 265.0, ["Specimen", "Strain (%)", "Stress (MPa)"]);
    let c = batch("Tensile batch C", 813, 1950.0, 240.0, ["Test piece", "Elongation (%)", "stress (MPa)"]);
    let linear = SavedReport::Linear {
        spec: LinearModelSpec {
            response: col(&a, "Stress (MPa)"),
            predictors: vec![col(&a, "Strain (%)")],
            effects: Vec::new(),
            splines: Vec::new(),
        },
        options: ReportOptions::default(),
    };
    let graph = stress_strain_graph(&a);
    ProjectFile {
        tables: vec![
            project_table("table-1", a, vec![graph, linear]),
            project_table("table-2", b, Vec::new()),
            project_table("table-3", c, Vec::new()),
        ],
        notes: "Invented tensile tests of three batches. Batch A's graph is formatted: right-click it ▸ Copy plot \
                style, then paste it onto other plots. Its ⋯ Options ▸ Duplicate on another table… copies the \
                graph or the model onto batch B (same column names) or batch C (named differently: choose \
                Elongation (%) for Strain (%)). File ▸ Save project as template… keeps the reports without the \
                data, and File ▸ New project from template… builds them on another batch or a folder of files."
            .to_owned(),
        ..ProjectFile::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_transfer::{ColumnMapping, duplicate_report, report_needs};

    #[test]
    fn batch_c_takes_batch_a_reports_after_one_choice() {
        let p = tensile_project();
        let (a, c) = (&p.tables[0], &p.tables[2]);
        assert_eq!(a.table.n_rows(), 84);
        for report in &a.reports {
            let mut m = ColumnMapping::new(report_needs(report, &a.table), &c.table);
            // "stress (MPa)" matches by case; "Strain (%)" needs choosing.
            assert!(!m.is_exact());
            for (need, chosen) in m.needs.iter().zip(m.chosen.iter_mut()) {
                if need.name == "Strain (%)" {
                    assert_eq!(*chosen, None);
                    *chosen = Some("Elongation (%)".into());
                }
                if need.name == "Specimen" {
                    *chosen = Some("Test piece".into());
                }
            }
            let pairs = m.pairs(&c.table).unwrap();
            duplicate_report(report, &a.table, &c.table, &pairs).unwrap();
        }
    }

    #[test]
    fn stress_rises_and_levels_off() {
        let t = tensile_batch_a();
        let stress = t.columns()[2].as_numeric().unwrap();
        // Specimen 1: about 200 MPa at 0.1 % strain, above yield at 2 %.
        assert!((stress[1] - 200.0).abs() < 15.0);
        assert!(stress[20] > 350.0 && stress[20] < 520.0, "{}", stress[20]);
    }
}

//! A sample project of two tables with a dashboard: the simulated plant
//! growth trial and the greenhouses' weekly climate, laid out as one
//! overview with filters shared by both tables.

use super::{bivariate, col, distribution, plant_growth, round_to, table};
use crate::dashboard::{
    Dashboard, DashboardId, DashboardTile, GridArea, SharedSwitcher, TextStyle, TileContent, TileId,
};
use crate::project::{ProjectFile, ProjectLayout, ProjectTable, SavedReport, TableId};
use crate::{DistributionOptions, LinearModelSpec, ReportOptions};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel, RowStateManager};
use stat_viz::FilterCondition;

/// A sample that opens as a project of several tables.
#[derive(Debug, Clone, Copy)]
pub struct ProjectSampleInfo {
    /// Menu name.
    pub name: &'static str,
    /// What the project holds and which features it shows.
    pub description: &'static str,
    /// Build the project: its tables, their reports and its dashboards.
    pub build: fn() -> ProjectFile,
}

/// Every sample project, in menu order.
#[must_use]
pub fn project_samples() -> Vec<ProjectSampleInfo> {
    vec![
        ProjectSampleInfo {
            name: "Greenhouse trial (simulated) — two tables and a dashboard",
            description: "A project of two tables (plant heights, and each greenhouse's weekly climate) with a \
                      dashboard: a heading, a note and four report tiles on a 12-column grid, filtered by \
                      greenhouse (both tables) and fertilizer (plants only; the climate tiles say they are not \
                      filtered by it), with a column picker on Humidity.",
            build: greenhouse_project,
        },
        ProjectSampleInfo {
            name: "Tensile tests (invented) — reuse reports and plot styles",
            description: "Three batches of invented tensile tests (stress against strain, four specimens each) in \
                      tables of the same layout. Batch A has a formatted stress–strain graph and a linear model; \
                      copy the graph's style onto other plots, duplicate either report onto batch B (same column \
                      names) or batch C (named differently, so the column mapping asks), or save the project as \
                      a template and build it on new data.",
            build: super::tensile_tests::tensile_project,
        },
        ProjectSampleInfo {
            name: "Clinical trial with dropout (simulated) — multiple imputation",
            description: "A two-arm trial of 120 patients with symptom scores at baseline and weeks 4, 8 and 12, \
                      where patients with bad scores drop out (missing at random). Its multiple imputation by \
                      chained equations (20 imputations, with trace, strip and density diagnostics), and the \
                      stacked table of the imputations with an analysis of covariance of week 12 on arm and \
                      baseline and a comparison of the arms, each pooled across the imputations by Rubin's \
                      rules and followed by its complete-case analysis.",
            build: super::clinical_dropout::clinical_dropout_project,
        },
    ]
}

/// **Simulated** weekly climate of the four greenhouses of the plant growth
/// trial over 12 weeks: mean day temperature (°C) = 21 + greenhouse offset
/// (0, −1.5, +0.8, +1.6) + 0.25 × week + N(0, 0.6²), and relative humidity
/// (%) = 62 − 1.2 × (temperature − 21) + N(0, 2.5²). Greenhouse is stored as
/// numbers 1–4 marked nominal, as in the plant table, so a filter on it
/// applies to both tables. Drawn with the workspace's integer-based
/// generator (seed 417), so it is the same on every platform.
#[must_use]
pub fn greenhouse_climate() -> DataTable {
    let mut rng = Rng::new(417);
    let offset = [0.0, -1.5, 0.8, 1.6];
    let (mut house, mut week, mut temp, mut humidity) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (g, off) in offset.iter().enumerate() {
        for w in 1..=12 {
            let t = 21.0 + off + 0.25 * f64::from(w) + 0.6 * rng.normal();
            let h = 62.0 - 1.2 * (t - 21.0) + 2.5 * rng.normal();
            house.push((g + 1) as f64);
            week.push(f64::from(w));
            temp.push(round_to(t, 1));
            humidity.push(round_to(h, 1));
        }
    }
    let mut greenhouse = Column::numeric("Greenhouse", house);
    greenhouse.level = MeasurementLevel::Nominal;
    let mut week = Column::numeric("Week", week);
    week.level = MeasurementLevel::Ordinal;
    table(
        "Greenhouse climate (simulated)",
        vec![greenhouse, week, Column::numeric("Temperature", temp), Column::numeric("Humidity", humidity)],
    )
}

fn project_table(id: &str, table: DataTable, reports: Vec<SavedReport>) -> ProjectTable {
    let rows = RowStateManager::new(table.n_rows());
    ProjectTable { id: TableId::from(id), table, rows, reports, external: None, lineage: None }
}

/// The greenhouse trial project: the plant growth table (a one-way model of
/// height by fertilizer and the distribution of height), the climate table
/// (humidity against temperature and the distribution of humidity), and the
/// dashboard "Greenhouse overview" showing copies of those four reports
/// under a heading and a note, filtered to greenhouses 1–3 and every
/// fertilizer, with a column picker offering Temperature and Week in
/// place of Humidity (showing Humidity). Its project notes describe the trial.
#[must_use]
pub fn greenhouse_project() -> ProjectFile {
    let plants = plant_growth();
    let climate = greenhouse_climate();
    let by_fertilizer = SavedReport::Linear {
        spec: LinearModelSpec {
            response: col(&plants, "Height"),
            predictors: vec![col(&plants, "Fertilizer")],
            effects: Vec::new(),
            splines: Vec::new(),
        },
        options: ReportOptions::default(),
    };
    let heights = distribution(&plants, "Height", DistributionOptions::default());
    let humidity_by_temperature = bivariate(&climate, "Temperature", "Humidity");
    let humidity = distribution(&climate, "Humidity", DistributionOptions::default());
    let report = |table: &str, report: &SavedReport| TileContent::Report {
        table: TableId::from(table),
        report: Box::new(report.clone()),
    };
    let tile = |id: u32, area: GridArea, content: TileContent| DashboardTile { id: TileId(id), area, content };
    let dashboard = Dashboard {
        id: DashboardId::from("dashboard-1"),
        name: "Greenhouse overview".to_owned(),
        grid_columns: crate::dashboard::DEFAULT_GRID_COLUMNS,
        tiles: vec![
            tile(
                1,
                GridArea::new(0, 0, 12, 1),
                TileContent::Text {
                    text: "Greenhouse trial: plants and climate".to_owned(),
                    style: TextStyle::Heading,
                },
            ),
            tile(
                2,
                GridArea::new(0, 1, 12, 1),
                TileContent::Text {
                    text: "Filters above apply to every tile whose table has the column: Greenhouse to both \
                           tables, Fertilizer to the plant tiles only. The Humidity switcher shows another climate \
                           column in every tile that reads Humidity. Drag a tile's strip to move it, its corner to \
                           resize it."
                        .to_owned(),
                    style: TextStyle::Body,
                },
            ),
            tile(3, GridArea::new(0, 2, 6, 6), report("table-1", &by_fertilizer)),
            tile(4, GridArea::new(6, 2, 6, 6), report("table-1", &heights)),
            tile(5, GridArea::new(0, 8, 6, 6), report("table-2", &humidity_by_temperature)),
            tile(6, GridArea::new(6, 8, 6, 6), report("table-2", &humidity)),
        ],
        filters: vec![
            FilterCondition::Levels {
                column: "Greenhouse".to_owned(),
                levels: vec!["1".to_owned(), "2".to_owned(), "3".to_owned()],
            },
            FilterCondition::Levels {
                column: "Fertilizer".to_owned(),
                levels: vec!["A".to_owned(), "B".to_owned(), "C".to_owned()],
            },
        ],
        switchers: vec![SharedSwitcher::new("Humidity", ["Temperature".to_owned(), "Week".to_owned()])],
    };
    ProjectFile {
        tables: vec![
            project_table("table-1", plants, vec![by_fertilizer, heights]),
            project_table("table-2", climate, vec![humidity_by_temperature, humidity]),
        ],
        scripts: Vec::new(),
        layout: ProjectLayout { active_dashboard: Some(DashboardId::from("dashboard-1")), ..ProjectLayout::default() },
        dashboards: vec![dashboard],
        notebooks: Vec::new(),
        name: None,
        folders: Vec::new(),
        notes: "Fertilizer trial: plant heights with the greenhouse climate log from the same weeks.\n\
                The dashboard links both tables; select plants or hours in any tile to see them everywhere."
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{load_project_file, save_project_file};

    #[test]
    fn the_greenhouse_project_is_valid_and_round_trips() {
        let file = greenhouse_project();
        let text = save_project_file(&file);
        let back = load_project_file(&text).expect("a valid project file");
        assert_eq!(back.tables.len(), 2);
        assert_eq!(back.dashboards, file.dashboards);
        assert_eq!(back.layout.active_dashboard, Some(DashboardId::from("dashboard-1")));
        let d = &back.dashboards[0];
        let results = crate::dashboard::compute_dashboard(&back, d);
        for t in &results.tiles {
            if let crate::dashboard::TileOutput::Report { report, .. } = &t.output {
                assert!(report.is_ok(), "{:?}: {report:?}", t.id);
            }
        }
        // Greenhouse 4 is filtered out of both tables; Fertilizer only applies to the plants.
        let plants = results.table(&TableId::from("table-1")).unwrap();
        assert_eq!(plants.filter.rows_kept(), 45);
        let climate = results.table(&TableId::from("table-2")).unwrap();
        assert_eq!(climate.filter.rows_kept(), 36);
        assert_eq!(climate.filter.skipped, vec!["Fertilizer".to_owned()]);
        // Switching Humidity to Week changes both climate tiles; the plant tiles do not read it.
        let mut d = d.clone();
        d.switchers[0] = d.switchers[0].showing("Week");
        let results = crate::dashboard::compute_dashboard(&back, &d);
        for t in &results.tiles {
            if let crate::dashboard::TileOutput::Report { report, switched, table } = &t.output {
                assert!(report.is_ok(), "{:?}: {report:?}", t.id);
                let expect = usize::from(table.as_str() == "table-2");
                assert_eq!(switched.len(), expect, "{:?}", t.id);
            }
        }
    }
}

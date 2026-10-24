//! Write the sample datasets as files in every supported text format, plus a
//! project, for testing import, export and project loading:
//!
//! ```text
//! cargo run -p app-model --example export_samples -- examples/data
//! ```

use app_model::file_formats::ExportFormat;
use app_model::project::{Project, save_project};
use app_model::sample;
use stat_data::{RowStateManager, write_delimited};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "examples/data".to_owned());
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir)?;
    let write = |name: &str, text: String| -> std::io::Result<()> {
        println!("wrote {}", dir.join(name).display());
        std::fs::write(dir.join(name), text)
    };
    // Plain data files cannot record measurement levels, so data with nominal
    // or ordinal number columns are saved as projects (data only), which keep them.
    let data_only = |table| {
        let rows = RowStateManager::new(stat_data::DataTable::n_rows(&table));
        save_project(&Project { origin: None, scripts: Vec::new(), external: None, table, rows, reports: Vec::new() })
    };
    write("plant_growth.acraft", data_only(sample::plant_growth()))?;
    write("customer_survey.acraft", data_only(sample::survey()))?;
    write("clinic_wait_times.tsv", ExportFormat::Tsv.write_text(&sample::wait_times()).unwrap_or_default())?;
    write("catalyst_experiment.json", ExportFormat::Json.write_text(&sample::catalyst()).unwrap_or_default())?;
    // Semicolon-separated text with a .txt name: the delimiter is detected.
    write("fill_weights_semicolon.txt", write_delimited(&sample::fill_weights(), ';'))?;
    write("michelson_speed_of_light.csv", ExportFormat::Csv.write_text(&sample::michelson()).unwrap_or_default())?;

    // A project with reports and row states: plant growth with the two
    // outliers excluded and selected rows coloured.
    let s = (sample::catalog()[2].build)();
    let mut rows = RowStateManager::new(s.table.n_rows());
    rows.set_excluded([16, 43], true);
    rows.select([16, 43], stat_data::SelectionMode::Replace);
    let project =
        Project { origin: None, scripts: Vec::new(), external: None, table: s.table, rows, reports: s.reports };
    write("plant_growth_outliers_excluded.acraft", save_project(&project))?;

    // Survival data: plain CSV (status 0/1 is an ordinary number column) and a
    // project that reopens with its survival report.
    // Stacked data: every value in one column, identified by Drug and Patient.
    write("sleep_drug_trial.csv", ExportFormat::Csv.write_text(&sample::sleep()).unwrap_or_default())?;
    write("bearing_life_test.csv", ExportFormat::Csv.write_text(&sample::bearings()).unwrap_or_default())?;
    let s = (sample::catalog()[13].build)();
    let rows = RowStateManager::new(s.table.n_rows());
    write(
        "leukemia_remission.acraft",
        save_project(&Project {
            origin: None,
            scripts: Vec::new(),
            external: None,
            table: s.table,
            rows,
            reports: s.reports,
        }),
    )?;
    Ok(())
}

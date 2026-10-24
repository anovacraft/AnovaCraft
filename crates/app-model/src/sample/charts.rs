//! Samples for the round, bridge, ridgeline and vector chart forms of the
//! graph canvas: an invented fortnight of harbour weather, an invented
//! household budget and invented soil textures.

use super::{Sample, table, text};
use crate::project::SavedReport;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_viz::{
    BarElement, BarLayout, EncodingPlan, GraphForm, GraphSpec, PieSettings, RadarSettings, RidgelineSettings,
    VectorSettings, WaterfallSettings, WindRoseSettings,
};

/// Round to `d` decimals.
fn round(x: f64, d: i32) -> f64 {
    let f = 10f64.powi(d);
    (x * f).round() / f
}

/// A saved graph of `form` with these shelves.
fn graph(form: GraphForm, x: &[&str], y: &[&str], color: Option<&str>) -> SavedReport {
    SavedReport::Graph {
        spec: Box::new(GraphSpec {
            encoding: EncodingPlan {
                x: x.iter().map(|s| (*s).to_owned()).collect(),
                y: y.iter().map(|s| (*s).to_owned()).collect(),
                color: color.map(str::to_owned),
                ..Default::default()
            },
            form,
            ..GraphSpec::default()
        }),
    }
}

/// An **invented** fortnight of three-hourly weather at a harbour (112
/// readings: 14 days × 8 times). The wind blows mostly from the south-west,
/// turning towards the west and freshening in the afternoon sea breeze;
/// temperature follows the day, humidity runs against it, and the sky is
/// called from humidity. East and North are the wind's components towards
/// the east and north (the direction is where the wind comes from, so a
/// south-westerly blows towards the north-east). Noise comes from the
/// workspace's integer-based generator (seed 2026) and values are rounded,
/// so the table is the same on every platform.
#[must_use]
pub fn harbour_weather() -> DataTable {
    let mut rng = Rng::new(2026);
    let mut noise = |half: usize| rng.below(2 * half + 1) as f64 - half as f64;
    let mut cols: [Vec<f64>; 9] = Default::default();
    let (mut part, mut sky) = (Vec::new(), Vec::new());
    for day in 1..=14 {
        let front = (f64::from(day) / 14.0 * std::f64::consts::TAU).sin();
        for k in 0..8 {
            let hour = 3 * k;
            let diurnal = ((f64::from(hour) - 9.0) / 24.0 * std::f64::consts::TAU).sin();
            let direction = (225.0 + 30.0 * diurnal + 25.0 * front + 3.0 * noise(10)).rem_euclid(360.0).round();
            let speed = round((4.5 + 2.5 * diurnal + 2.0 * front + 0.2 * noise(8)).max(0.3), 1);
            let towards = (direction + 180.0).to_radians();
            let temperature = round(13.0 + 4.0 * diurnal - 1.5 * front + 0.1 * noise(8), 1);
            let humidity = (88.0 - 2.5 * (temperature - 13.0) + 4.0 * front + noise(5)).clamp(40.0, 100.0).round();
            let pressure = round(1013.0 - 7.0 * front + 0.1 * noise(10), 1);
            cols[0].push(f64::from(day));
            cols[1].push(f64::from(hour));
            cols[2].push(direction);
            cols[3].push(speed);
            cols[4].push(round(speed * towards.sin(), 2));
            cols[5].push(round(speed * towards.cos(), 2));
            cols[6].push(temperature);
            cols[7].push(humidity);
            cols[8].push(pressure);
            part.push(
                ["Night", "Night", "Morning", "Morning", "Afternoon", "Afternoon", "Evening", "Evening"][k as usize]
                    .to_owned(),
            );
            sky.push(
                match humidity {
                    h if h >= 96.0 && hour < 12 => "Fog",
                    h if h >= 90.0 => "Rain",
                    h if h >= 80.0 => "Cloudy",
                    _ => "Clear",
                }
                .to_owned(),
            );
        }
    }
    let names = [
        "Day",
        "Hour",
        "Wind direction (°)",
        "Wind speed (m/s)",
        "East (m/s)",
        "North (m/s)",
        "Temperature (°C)",
        "Humidity (%)",
        "Pressure (hPa)",
    ];
    let mut columns: Vec<Column> = names.iter().zip(cols).map(|(n, v)| Column::numeric(*n, v)).collect();
    columns.insert(2, text("Time of day", part));
    columns.push(text("Sky", sky));
    table("Harbour weather (invented)", columns)
}

/// The harbour weather with a wind rose, a vector field of the wind through
/// the fortnight, a radar chart of the means by time of day, temperature
/// ridgelines, and a donut chart of the sky.
pub(super) fn harbour_weather_sample() -> Sample {
    let means = RadarSettings { group_means: true, fill: true };
    let donut = PieSettings { donut: true, ..PieSettings::default() };
    Sample {
        reports: vec![
            graph(
                GraphForm::WindRose(WindRoseSettings::default()),
                &["Wind direction (°)"],
                &["Wind speed (m/s)"],
                None,
            ),
            graph(
                GraphForm::VectorField(VectorSettings::default()),
                &["Hour", "East (m/s)"],
                &["Day", "North (m/s)"],
                None,
            ),
            graph(
                GraphForm::Radar(means),
                &["Wind speed (m/s)", "Temperature (°C)"],
                &["Humidity (%)", "Pressure (hPa)"],
                Some("Time of day"),
            ),
            graph(
                GraphForm::Ridgeline(RidgelineSettings::default()),
                &["Temperature (°C)"],
                &["Time of day"],
                Some("Time of day"),
            ),
            graph(GraphForm::Pie(donut), &["Sky"], &[], None),
        ],
        table: harbour_weather(),
    }
}

/// An **invented** quarter of a household budget: each item's amount in
/// January, February and March (income positive, spending negative), in the
/// order the items are listed, with its category and the amount spent (zero
/// for income).
#[must_use]
pub fn household_budget() -> DataTable {
    let items: [(&str, &str, [f64; 3]); 9] = [
        ("Salary", "Income", [3200.0, 3200.0, 3350.0]),
        ("Side work", "Income", [420.0, 0.0, 610.0]),
        ("Rent", "Housing", [-1250.0, -1250.0, -1250.0]),
        ("Utilities", "Housing", [-230.0, -255.0, -190.0]),
        ("Groceries", "Food", [-540.0, -505.0, -560.0]),
        ("Eating out", "Food", [-140.0, -95.0, -170.0]),
        ("Transport", "Travel", [-180.0, -175.0, -210.0]),
        ("Insurance", "Housing", [-95.0, -95.0, -95.0]),
        ("Leisure", "Leisure", [-260.0, -120.0, -330.0]),
    ];
    let (mut month, mut item, mut category, mut amount, mut spent) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (m, name) in ["January", "February", "March"].into_iter().enumerate() {
        for (i, c, a) in &items {
            month.push(name.to_owned());
            item.push((*i).to_owned());
            category.push((*c).to_owned());
            amount.push(a[m]);
            spent.push((-a[m]).max(0.0));
        }
    }
    table(
        "Household budget (invented)",
        vec![
            text("Month", month),
            text("Item", item),
            text("Category", category),
            Column::numeric("Amount", amount),
            Column::numeric("Spent", spent),
        ],
    )
}

/// The budget as a waterfall chart from income to what is left, a donut of
/// spending by category, and lollipops of each item's monthly mean.
pub(super) fn household_budget_sample() -> Sample {
    let by_sum = PieSettings { donut: true, size_by_sum: true, labels: true };
    let lollipops = SavedReport::Graph {
        spec: Box::new(GraphSpec {
            encoding: EncodingPlan { x: vec!["Item".into()], y: vec!["Amount".into()], ..Default::default() },
            bars: Some(BarElement { layout: BarLayout::Lollipop, labels: true, ..BarElement::default() }),
            points: None,
            ..GraphSpec::default()
        }),
    };
    Sample {
        reports: vec![
            graph(GraphForm::Waterfall(WaterfallSettings::default()), &["Item"], &["Amount"], None),
            graph(GraphForm::Pie(by_sum), &["Category"], &["Spent"], None),
            lollipops,
        ],
        table: household_budget(),
    }
}

/// **Invented** textures of 48 soil samples from three fields: the sand,
/// silt and clay percentages of each (summing to 100) and the field. Each
/// field has its own typical texture, spread by the workspace's
/// integer-based generator (seed 77).
#[must_use]
pub fn soil_texture() -> DataTable {
    let mut rng = Rng::new(77);
    let fields: [(&str, [f64; 3]); 3] =
        [("River flat", [25.0, 50.0, 25.0]), ("Upper terrace", [60.0, 25.0, 15.0]), ("Basin", [15.0, 35.0, 50.0])];
    let (mut field, mut sand, mut silt, mut clay) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (name, centre) in fields {
        for _ in 0..16 {
            let s = (centre[0] + rng.below(21) as f64 - 10.0).max(1.0);
            let c = (centre[2] + rng.below(15) as f64 - 7.0).max(1.0);
            let t = (100.0 - s - c).max(1.0);
            let total = s + t + c;
            let (s, c) = ((100.0 * s / total).round(), (100.0 * c / total).round());
            field.push(name.to_owned());
            sand.push(s);
            clay.push(c);
            silt.push(100.0 - s - c);
        }
    }
    table(
        "Soil texture (invented)",
        vec![
            text("Field", field),
            Column::numeric("Sand (%)", sand),
            Column::numeric("Silt (%)", silt),
            Column::numeric("Clay (%)", clay),
        ],
    )
}

/// The soil textures on a ternary plot, coloured and shaped by field.
pub(super) fn soil_texture_sample() -> Sample {
    Sample {
        reports: vec![graph(GraphForm::Ternary, &["Sand (%)", "Silt (%)"], &["Clay (%)"], Some("Field"))],
        table: soil_texture(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::RowStateManager;

    #[test]
    fn weather_components_match_direction_and_speed() {
        let t = harbour_weather();
        assert_eq!(t.n_rows(), 112);
        let num = |c: &str| t.column(t.find(c).unwrap()).unwrap().as_numeric().unwrap().to_vec();
        let (d, s, e, n) = (num("Wind direction (°)"), num("Wind speed (m/s)"), num("East (m/s)"), num("North (m/s)"));
        for r in 0..t.n_rows() {
            assert!((e[r].hypot(n[r]) - s[r]).abs() < 0.02, "row {r}");
            // A wind from direction d blows towards d + 180°.
            let towards = e[r].atan2(n[r]).to_degrees().rem_euclid(360.0);
            let diff = (towards - (d[r] + 180.0).rem_euclid(360.0)).abs();
            assert!(diff.min(360.0 - diff) < 1.0, "row {r}");
        }
    }

    #[test]
    fn budget_and_soil_samples_draw() {
        for sample in [harbour_weather_sample(), household_budget_sample(), soil_texture_sample()] {
            let rows = RowStateManager::new(sample.table.n_rows());
            for r in &sample.reports {
                let SavedReport::Graph { spec } = r else { panic!() };
                let frame = spec.frame(&sample.table, &rows).unwrap();
                let layers = spec.layers(&frame, &rows);
                assert!(layers.iter().flatten().any(|l| !l.marks.is_empty()), "{:?}", spec.form);
                assert!(frame.notes.iter().all(|n| !n.contains("left out")), "{:?}: {:?}", spec.form, frame.notes);
            }
        }
        let soil = soil_texture();
        let num = |c: &str| soil.column(soil.find(c).unwrap()).unwrap().as_numeric().unwrap().to_vec();
        let (a, b, c) = (num("Sand (%)"), num("Silt (%)"), num("Clay (%)"));
        assert!((0..soil.n_rows()).all(|r| a[r] + b[r] + c[r] == 100.0 && a[r].min(b[r]).min(c[r]) > 0.0));
    }
}

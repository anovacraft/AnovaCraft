//! NIST StRD one-way analysis-of-variance datasets.
//!
//! * **SiRstv** — resistivity of silicon wafers measured by five instruments,
//!   five readings each (lower difficulty), transcribed from the published
//!   listing.
//! * **SmLs01 – SmLs09** — the Simon–Lesage synthetic datasets with 9
//!   treatments of 21 (SmLs01, 04, 07), 201 (SmLs02, 05, 08) or 2001 (SmLs03,
//!   06, 09) replicates, at lower (01–03), average (04–06) and higher (07–09)
//!   difficulty. They are generated here from their published construction:
//!   treatment 1 is centred on 1.4 and treatments 2–9 alternate between centres
//!   1.3 and 1.5; each treatment's first value is its centre and the remaining
//!   r − 1 alternate centre − 0.1, centre + 0.1. SmLs04–06 add 10⁶ and SmLs07–09
//!   add 10¹² to every value. With r replicates the construction gives the
//!   certified sums of squares exactly (8r · 0.01 between, 9(r − 1) · 0.01
//!   within; for SmLs03, 160.08 and 180), F = r and R² = 8r / (17r − 9), and the
//!   offsets do not change any certified value. Each certified value is the
//!   binary64 number nearest that exact rational.
//!
//! AtmWtAg (NIST's twelfth ANOVA problem, real measurements) cannot be
//! generated; NIST's file is bundled as `data/anova/AtmWtAg.dat` and read by
//! [`parse_strd_anova`].
//!
//! Values are produced by parsing their decimal text, so each is the binary64
//! number nearest the published decimal, exactly as a reader of NIST's files
//! would obtain.

use crate::Difficulty;

/// Certified one-way ANOVA results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CertifiedAnova {
    /// Between-groups degrees of freedom.
    pub df_between: usize,
    /// Within-groups degrees of freedom.
    pub df_within: usize,
    /// Between-groups sum of squares.
    pub ss_between: f64,
    /// Within-groups sum of squares.
    pub ss_within: f64,
    /// Between-groups mean square.
    pub ms_between: f64,
    /// Within-groups mean square.
    pub ms_within: f64,
    /// F statistic.
    pub f_statistic: f64,
    /// R².
    pub r_squared: f64,
    /// Residual standard deviation.
    pub residual_sd: f64,
}

/// One ANOVA reference problem.
#[derive(Debug, Clone, PartialEq)]
pub struct AnovaDataset {
    /// NIST dataset name.
    pub name: &'static str,
    /// NIST difficulty rating.
    pub difficulty: Difficulty,
    /// Observations, one vector per group.
    pub groups: Vec<Vec<f64>>,
    /// Certified results.
    pub certified: CertifiedAnova,
}

/// The bundled ANOVA datasets, ordered by difficulty.
#[must_use]
pub fn anova_datasets() -> Vec<AnovaDataset> {
    vec![
        sirstv(),
        simon_lesage("SmLs01", 0, 21, Difficulty::Lower),
        simon_lesage("SmLs02", 0, 201, Difficulty::Lower),
        simon_lesage("SmLs03", 0, 2001, Difficulty::Lower),
        simon_lesage("SmLs04", 1_000_000, 21, Difficulty::Average),
        simon_lesage("SmLs05", 1_000_000, 201, Difficulty::Average),
        simon_lesage("SmLs06", 1_000_000, 2001, Difficulty::Average),
        simon_lesage("SmLs07", 1_000_000_000_000, 21, Difficulty::Higher),
        simon_lesage("SmLs08", 1_000_000_000_000, 201, Difficulty::Higher),
        simon_lesage("SmLs09", 1_000_000_000_000, 2001, Difficulty::Higher),
    ]
}

fn sirstv() -> AnovaDataset {
    #[rustfmt::skip]
    const DATA: [[f64; 5]; 5] = [
        [196.3052, 196.1240, 196.1890, 196.2569, 196.3403],
        [196.3042, 196.3825, 196.1669, 196.3257, 196.0422],
        [196.1303, 196.2005, 196.2889, 196.0343, 196.1811],
        [196.2795, 196.1748, 196.1494, 196.1485, 195.9885],
        [196.2119, 196.1051, 196.1850, 196.0052, 196.2090],
    ];
    AnovaDataset {
        name: "SiRstv",
        difficulty: Difficulty::Lower,
        groups: DATA.iter().map(|g| g.to_vec()).collect(),
        certified: CertifiedAnova {
            df_between: 4,
            df_within: 20,
            ss_between: 5.114_626_160_000_00e-2,
            ss_within: 2.166_365_600_000_00e-1,
            ms_between: 1.278_656_540_000_00e-2,
            ms_within: 1.083_182_800_000_00e-2,
            f_statistic: 1.180_462_374_402_55,
            r_squared: 1.909_990_390_511_29e-1,
            residual_sd: 1.040_760_683_346_56e-1,
        },
    }
}

/// Simon–Lesage construction with `replicates` values per treatment and
/// `offset` added to every value.
fn simon_lesage(name: &'static str, offset: i64, replicates: usize, difficulty: Difficulty) -> AnovaDataset {
    // Values are written as tenths: centre 14, 13 or 15; deviations ±1 tenth.
    let decimal = |tenths: i64| -> f64 {
        let text = format!("{}.{}", offset + tenths / 10, tenths % 10);
        text.parse().expect("generated decimal text is valid")
    };
    let groups = (0..9)
        .map(|g| {
            let centre: i64 = if g == 0 {
                14
            } else if g % 2 == 1 {
                13
            } else {
                15
            };
            std::iter::once(centre)
                .chain((0..replicates - 1).map(|j| if j % 2 == 0 { centre - 1 } else { centre + 1 }))
                .map(decimal)
                .collect()
        })
        .collect();
    AnovaDataset { name, difficulty, groups, certified: simon_lesage_certified(replicates) }
}

/// Certified values of a Simon–Lesage dataset with `r` replicates per
/// treatment. Each is a single correctly rounded operation on exact integers,
/// so it is the binary64 number nearest the exact value.
#[allow(clippy::cast_precision_loss)] // r is at most 2001
fn simon_lesage_certified(r: usize) -> CertifiedAnova {
    let r = r as f64;
    CertifiedAnova {
        df_between: 8,
        df_within: 9 * (r as usize - 1),
        ss_between: 8.0 * r / 100.0,
        ss_within: 9.0 * (r - 1.0) / 100.0,
        ms_between: r / 100.0,
        ms_within: 1.0 / 100.0,
        f_statistic: r,
        r_squared: 8.0 * r / (17.0 * r - 9.0),
        residual_sd: 1.0 / 10.0,
    }
}

/// Parse a NIST StRD one-way ANOVA `.dat` file (such as AtmWtAg) into a
/// dataset named `name`. Recognised lines (after trimming): `Between <label>
/// <df> <SS> <MS> <F>`, `Within <label> <df> <SS> <MS>`, a line ending in
/// `R-Squared <value>`, a line ending in `Standard Deviation <value>`, and
/// the data heading `Data: <group label> <value label>`, after which every
/// non-blank line is a group number and an observation. Other lines, such as
/// the description's `Data: 1 Factor`, are skipped. Groups are numbered from 1 in the file.
///
/// # Errors
/// A description of the first missing or malformed section.
pub fn parse_strd_anova(name: &'static str, difficulty: Difficulty, text: &str) -> Result<AnovaDataset, String> {
    let num = |s: &str| s.parse::<f64>().ok();
    let tail = |tokens: &[&str], k: usize| -> Option<Vec<f64>> {
        (tokens.len() >= k).then(|| tokens[tokens.len() - k..].iter().map(|t| num(t)).collect()).flatten()
    };
    let (mut between, mut within, mut r_squared, mut residual_sd) = (None, None, None, None);
    let mut groups: Vec<Vec<f64>> = Vec::new();
    let mut in_data = false;
    for (line_no, raw) in text.lines().enumerate() {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if in_data {
            match tokens[..] {
                [] => {}
                [g, y] => {
                    let (Some(g), Some(y)) = (num(g), num(y)) else {
                        return Err(format!("line {}: expected a group number and a value", line_no + 1));
                    };
                    if g < 1.0 || g.fract() != 0.0 || g > 1000.0 {
                        return Err(format!("line {}: group {g} is not a number from 1", line_no + 1));
                    }
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // checked above
                    let g = g as usize - 1;
                    if groups.len() <= g {
                        groups.resize(g + 1, Vec::new());
                    }
                    groups[g].push(y);
                }
                _ => return Err(format!("line {}: expected two fields", line_no + 1)),
            }
            continue;
        }
        match tokens.first().copied() {
            Some("Between") => between = tail(&tokens, 4),
            Some("Within") => within = tail(&tokens, 3),
            // The data heading names two columns ("Data: Instrument AgWt"); the
            // description above it also starts with "Data:" ("Data: 1 Factor").
            Some("Data:") if tokens.len() == 3 && tokens[1..].iter().all(|t| num(t).is_none()) => in_data = true,
            _ if raw.contains("R-Squared") => r_squared = tail(&tokens, 1).map(|v| v[0]),
            _ if raw.contains("Standard Deviation") => residual_sd = tail(&tokens, 1).map(|v| v[0]),
            _ => {}
        }
    }
    let between = between.ok_or("no Between row")?;
    let within = within.ok_or("no Within row")?;
    if groups.is_empty() || groups.iter().any(Vec::is_empty) {
        return Err("no data, or a group with no observations".into());
    }
    let df = |x: f64| -> Result<usize, String> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // checked integral and positive
        (x >= 0.0 && x.fract() == 0.0).then_some(x as usize).ok_or_else(|| format!("bad df {x}"))
    };
    Ok(AnovaDataset {
        name,
        difficulty,
        groups,
        certified: CertifiedAnova {
            df_between: df(between[0])?,
            df_within: df(within[0])?,
            ss_between: between[1],
            ss_within: within[1],
            ms_between: between[2],
            ms_within: within[2],
            f_statistic: between[3],
            r_squared: r_squared.ok_or("no R-Squared")?,
            residual_sd: residual_sd.ok_or("no Standard Deviation")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        let all = anova_datasets();
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].groups.iter().map(Vec::len).sum::<usize>(), 25);
        let sizes = [21, 201, 2001].repeat(3);
        for (d, r) in all[1..].iter().zip(sizes) {
            assert_eq!(d.groups.len(), 9, "{}", d.name);
            assert!(d.groups.iter().all(|g| g.len() == r), "{}", d.name);
        }
        assert_eq!(all[1].groups[0][0], 1.4);
        assert_eq!(all[1].groups[1][..3], [1.3, 1.2, 1.4]);
        assert_eq!(all[6].groups[2][1], 1_000_001.4);
        assert_eq!(all[9].groups[0][0], 1_000_000_000_001.4);
    }

    /// The generated certified values equal NIST's published decimals (as
    /// listed in the SmLs01, SmLs02 and SmLs03 certificates).
    #[test]
    fn certified_values_match_the_published_decimals() {
        let c = simon_lesage_certified(21);
        assert_eq!((c.ss_between, c.ss_within, c.f_statistic), (1.68, 1.8, 21.0));
        assert_eq!((c.ms_between, c.ms_within, c.residual_sd), (0.21, 0.01, 0.1));
        assert!((c.r_squared - 0.482_758_620_689_655).abs() < 1e-15);
        let c = simon_lesage_certified(201);
        assert_eq!((c.ss_between, c.ss_within, c.ms_between, c.f_statistic), (16.08, 18.0, 2.01, 201.0));
        assert!((c.r_squared - 0.471_830_985_915_493).abs() < 1e-15);
        let c = simon_lesage_certified(2001);
        assert_eq!((c.ss_between, c.ss_within, c.ms_between, c.f_statistic), (160.08, 180.0, 20.01, 2001.0));
        assert!((c.r_squared - 4.707_127_734_650_67e-1).abs() < 1e-15);
    }

    /// SiRstv written out in NIST's ANOVA file layout parses back to the
    /// transcribed dataset, so the parser is exercised without NIST's files.
    #[test]
    fn parses_the_nist_anova_layout() {
        let d = sirstv();
        let c = d.certified;
        let mut text = format!(
            "NIST/ITL StRD\nDataset Name:  SiRstv\n\nData:           1 Factor\n                5 Treatments\n\nCertified Values:\n\nSource of                  Sums of               Mean               F Statistic\n\
             Variation          df      Squares              Squares\n\n\
             Between Instrument  {} {:e} {:e} {:e}\nWithin Instrument  {} {:e} {:e}\n\n\
             Certified R-Squared {:e}\n\nCertified Residual\nStandard Deviation  {:e}\n\n\
             Data:   Instrument   Resistance\n",
            c.df_between,
            c.ss_between,
            c.ms_between,
            c.f_statistic,
            c.df_within,
            c.ss_within,
            c.ms_within,
            c.r_squared,
            c.residual_sd,
        );
        for (g, values) in d.groups.iter().enumerate() {
            for v in values {
                text.push_str(&format!("           {}            {v}\n", g + 1));
            }
        }
        let parsed = parse_strd_anova("SiRstv", Difficulty::Lower, &text).unwrap();
        assert_eq!(parsed, d);
        assert!(parse_strd_anova("x", Difficulty::Lower, "Between A 1 2 3 4\n").is_err());
        assert!(
            parse_strd_anova("x", Difficulty::Lower, &text.replace("Resistance\n", "Resistance\n 0 1.0\n")).is_err()
        );
    }

    /// The certified values are internally consistent with the construction.
    #[test]
    fn certified_values_are_consistent() {
        for d in anova_datasets() {
            let c = d.certified;
            let n: usize = d.groups.iter().map(Vec::len).sum();
            assert_eq!(c.df_between, d.groups.len() - 1);
            assert_eq!(c.df_within, n - d.groups.len());
            assert!((c.ss_between / c.df_between as f64 - c.ms_between).abs() <= 1e-14 * c.ms_between);
            assert!((c.ss_within / c.df_within as f64 - c.ms_within).abs() <= 1e-14 * c.ms_within);
            assert!((c.ms_between / c.ms_within - c.f_statistic).abs() <= 1e-13 * c.f_statistic);
            assert!((c.ss_between / (c.ss_between + c.ss_within) - c.r_squared).abs() <= 1e-14);
            assert!((c.ms_within.sqrt() - c.residual_sd).abs() <= 1e-14);
        }
    }
}

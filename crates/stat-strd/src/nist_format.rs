//! Parser for NIST StRD linear-regression `.dat` files.
//!
//! These files have a free-text header containing the certified values and a
//! whitespace-separated data block. Recognised lines (after trimming):
//!
//! * `B<k>  <estimate>  <std dev>` — a certified parameter;
//! * `Standard Deviation  <value>` — residual standard deviation;
//! * `R-Squared  <value>`;
//! * `Regression  <df> <SS> <MS> <F>` and `Residual  <df> <SS> <MS>` — ANOVA rows;
//! * `Data:  y  x …` (column names only) — every following non-blank line is a
//!   data row.
//!
//! Numbers use Fortran-style exponents (`0.12E-03`), which Rust parses directly.
//! Bundling NIST's original files and parsing them avoids transcription errors.

use crate::CertifiedValues;

/// Certified values and raw data rows parsed from a `.dat` file.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedStrd {
    /// Certified results. `f_statistic` is `None` if the file gives none.
    pub certified: CertifiedValues,
    /// Data rows in file column order (response first).
    pub rows: Vec<Vec<f64>>,
}

/// Parse a NIST StRD linear-regression file.
///
/// # Errors
/// Returns a description of the first missing or malformed section.
pub fn parse_strd_linear(text: &str) -> Result<ParsedStrd, String> {
    let num = |s: &str| s.parse::<f64>().ok();
    let mut parameters: Vec<(usize, f64, f64)> = Vec::new();
    let (mut residual_sd, mut r_squared) = (None, None);
    let (mut regression, mut residual) = (None, None);
    let mut rows = Vec::new();
    let mut in_data = false;

    for (line_no, raw) in text.lines().enumerate() {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        if in_data {
            let row: Option<Vec<f64>> = tokens.iter().map(|t| num(t)).collect();
            rows.push(row.ok_or_else(|| format!("line {}: malformed data row", line_no + 1))?);
            continue;
        }
        match tokens.as_slice() {
            ["Data:", names @ ..] if !names.is_empty() && names.iter().all(|t| num(t).is_none()) => in_data = true,
            [name, est, sd] if name.len() > 1 && name.starts_with('B') => {
                if let (Ok(k), Some(e), Some(s)) = (name[1..].parse::<usize>(), num(est), num(sd)) {
                    parameters.push((k, e, s));
                }
            }
            ["Standard", "Deviation", v] => residual_sd = num(v),
            ["R-Squared", v] => r_squared = num(v),
            ["Regression", df, ss, _ms, f] => regression = Some((parse_df(df)?, num(ss), num(f))),
            ["Residual", df, ss, _ms] => residual = Some((parse_df(df)?, num(ss))),
            _ => {}
        }
    }

    // Parameters are numbered B0..Bk, or B1..Bk for models without an intercept.
    parameters.sort_by_key(|p| p.0);
    let first = parameters.first().map_or(0, |p| p.0);
    if parameters.is_empty() || first > 1 || parameters.iter().enumerate().any(|(i, p)| p.0 != i + first) {
        return Err("certified parameters missing or not consecutively numbered".into());
    }
    let (df_regression, ss_regression, f_statistic) = regression.ok_or("missing Regression ANOVA row")?;
    let (df_residual, ss_residual) = residual.ok_or("missing Residual ANOVA row")?;
    if rows.is_empty() {
        return Err("missing data block".into());
    }
    Ok(ParsedStrd {
        certified: CertifiedValues {
            parameters: parameters.into_iter().map(|p| (p.1, p.2)).collect(),
            residual_sd: residual_sd.ok_or("missing residual standard deviation")?,
            r_squared: r_squared.ok_or("missing R-Squared")?,
            df_regression,
            df_residual,
            ss_regression: ss_regression.ok_or("malformed regression sum of squares")?,
            ss_residual: ss_residual.ok_or("malformed residual sum of squares")?,
            f_statistic,
        },
        rows,
    })
}

fn parse_df(s: &str) -> Result<usize, String> {
    s.parse().map_err(|_| format!("malformed degrees of freedom {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "NIST/ITL StRD\r\nData:          1 Response Variable (y)\r\n\
        \r\n     Parameter         Estimate                of Estimate\r\n\
        \x20       B0        -1.5E+00         0.25\r\n        B1        2.0         0.5E-01\r\n\
        \x20    Residual\r\n     Standard Deviation   0.3E-02\r\n     R-Squared            0.99\r\n\
        Regression    1     4.0     4.0  12.5\r\nResidual      2     0.64    0.32\r\n\
        Data:          y           x\r\n   1.0  2.0\r\n   3.5 -4.25\r\n";

    #[test]
    fn parses_sections_and_crlf() {
        let p = parse_strd_linear(SAMPLE).unwrap();
        assert_eq!(p.certified.parameters, vec![(-1.5, 0.25), (2.0, 0.05)]);
        assert_eq!(p.certified.residual_sd, 0.003);
        assert_eq!(p.certified.r_squared, 0.99);
        assert_eq!((p.certified.df_regression, p.certified.df_residual), (1, 2));
        assert_eq!(p.certified.f_statistic, Some(12.5));
        assert_eq!(p.rows, vec![vec![1.0, 2.0], vec![3.5, -4.25]]);
    }

    #[test]
    fn reports_missing_sections() {
        assert!(parse_strd_linear("Data:  y x\n1 2\n").is_err());
        let no_data = SAMPLE.split("Data:          y").next().unwrap();
        assert!(parse_strd_linear(no_data).is_err());
    }
}

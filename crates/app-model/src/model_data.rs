//! Shared preparation of a response and predictors for model fitting: picks
//! the complete, included rows and codes continuous and categorical
//! predictors with [`stat_linear::ModelDesign`].

use stat_core::{Matrix, Result, StatError};
use stat_data::{DataTable, MeasurementLevel, RowStateManager};
use stat_linear::{ModelDesign, Predictor, PredictorValues, TermKind, TermValue, natural_order};

/// How a predictor's values are held.
#[derive(Debug, Clone)]
pub(crate) enum PredictorData {
    Continuous(Vec<f64>),
    Categorical(Vec<String>),
}

/// A two-level response coded 0/1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BinaryCoding {
    /// Level coded 0.
    pub other: String,
    /// Level coded 1 (the modelled event).
    pub event: String,
}

/// How [`ModelData`] reads the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coding<'a> {
    /// Numbers as they are.
    Numeric,
    /// Exactly two values, coded 0/1: the given event is 1, or without one
    /// the second value in natural order.
    Binary(Option<&'a str>),
    /// Category codes, in this order first.
    Categorical(&'a [String]),
}

/// Response and predictors restricted to complete, included rows.
#[derive(Debug, Clone)]
pub(crate) struct ModelData {
    pub response_name: String,
    /// Response for every dataset row (NaN when missing).
    pub y_all: Vec<f64>,
    pub binary: Option<BinaryCoding>,
    /// Categories of a categorical response (see
    /// [`ModelData::gather_categorical`]), whose codes `y_all` holds; empty
    /// otherwise.
    pub categories: Vec<String>,
    pub predictor_names: Vec<String>,
    pub predictors: Vec<PredictorData>,
    /// The predictor columns, for writing prediction formulas.
    pub factors: Vec<crate::prediction_formula::FactorColumn>,
    pub rows_used: Vec<usize>,
    pub n_missing: usize,
    pub n_excluded: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

impl ModelData {
    /// Gather the data. With `binary`, the response must have exactly two
    /// distinct values (numbers 0/1, or any two labels; the second in natural
    /// order is the event).
    pub fn gather(
        table: &DataTable,
        states: &RowStateManager,
        response: usize,
        predictors: &[usize],
        binary: bool,
    ) -> Result<Self> {
        if predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        Self::gather_any(table, states, response, predictors, binary)
    }

    /// [`ModelData::gather`] allowing no predictors (an intercept-only model).
    pub fn gather_any(
        table: &DataTable,
        states: &RowStateManager,
        response: usize,
        predictors: &[usize],
        binary: bool,
    ) -> Result<Self> {
        Self::gather_coded(
            table,
            states,
            response,
            predictors,
            if binary { Coding::Binary(None) } else { Coding::Numeric },
        )
    }

    /// Gather a two-valued response with `event` (one of its values) coded 1,
    /// or without one the second value in natural order, as
    /// [`ModelData::gather`] with `binary` does.
    ///
    /// # Errors
    /// As [`ModelData::gather`]; also when `event` is not one of the
    /// response's two values in the rows a fit can use.
    pub fn gather_binary(
        table: &DataTable,
        states: &RowStateManager,
        response: usize,
        predictors: &[usize],
        event: Option<&str>,
    ) -> Result<Self> {
        if predictors.is_empty() {
            return Err(invalid("choose at least one predictor"));
        }
        Self::gather_any_binary(table, states, response, predictors, event)
    }

    /// [`ModelData::gather_binary`] allowing no predictors.
    pub fn gather_any_binary(
        table: &DataTable,
        states: &RowStateManager,
        response: usize,
        predictors: &[usize],
        event: Option<&str>,
    ) -> Result<Self> {
        Self::gather_coded(table, states, response, predictors, Coding::Binary(event))
    }

    /// Gather a categorical response: its categories are the distinct values
    /// of the rows a fit can use (included, every predictor present), in the
    /// order `order` gives (categories it does not list follow in natural
    /// order; an empty `order` means natural order), and `y_all` holds each
    /// row's category code (NaN when missing or not a category of those rows).
    pub fn gather_categorical(
        table: &DataTable,
        states: &RowStateManager,
        response: usize,
        predictors: &[usize],
        order: &[String],
    ) -> Result<Self> {
        Self::gather_coded(table, states, response, predictors, Coding::Categorical(order))
    }

    /// Gather predictors for a response that is not one numeric column (a
    /// survival time with its censoring, or an inspection interval): `y_all`
    /// holds one value per table row, NaN where the response is unusable, and
    /// `response_name` names it.
    pub fn gather_with_response(
        table: &DataTable,
        states: &RowStateManager,
        response_name: String,
        y_all: Vec<f64>,
        predictors: &[usize],
    ) -> Result<Self> {
        if y_all.len() != table.n_rows() {
            return Err(invalid("the response needs one value per row"));
        }
        let mut names = Vec::new();
        let mut data = Vec::new();
        for &p in predictors {
            let c = table.column(p).ok_or_else(|| invalid(format!("no column {p}")))?;
            names.push(c.name.clone());
            data.push(match (c.as_numeric(), c.level) {
                (Some(v), MeasurementLevel::Continuous) => PredictorData::Continuous(v.to_vec()),
                _ => PredictorData::Categorical((0..table.n_rows()).map(|r| c.level_text(r)).collect()),
            });
        }
        let present = |d: &PredictorData, r: usize| match d {
            PredictorData::Continuous(v) => v[r].is_finite(),
            PredictorData::Categorical(v) => !v[r].trim().is_empty(),
        };
        let complete: Vec<usize> =
            (0..table.n_rows()).filter(|&r| y_all[r].is_finite() && data.iter().all(|d| present(d, r))).collect();
        let rows_used: Vec<usize> = complete.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        Ok(Self {
            response_name,
            n_missing: table.n_rows() - complete.len(),
            n_excluded: complete.len() - rows_used.len(),
            y_all,
            binary: None,
            categories: Vec::new(),
            predictor_names: names,
            predictors: data,
            factors: crate::prediction_formula::factor_columns(table, predictors),
            rows_used,
        })
    }

    fn gather_coded(
        table: &DataTable,
        states: &RowStateManager,
        response: usize,
        predictors: &[usize],
        coding: Coding<'_>,
    ) -> Result<Self> {
        if predictors.contains(&response) {
            return Err(invalid("the response cannot also be a predictor"));
        }
        let col = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let rc = col(response)?;
        let mut names = Vec::new();
        let mut data = Vec::new();
        for &p in predictors {
            let c = col(p)?;
            names.push(c.name.clone());
            data.push(match (c.as_numeric(), c.level) {
                (Some(v), MeasurementLevel::Continuous) => PredictorData::Continuous(v.to_vec()),
                _ => PredictorData::Categorical((0..table.n_rows()).map(|r| c.level_text(r)).collect()),
            });
        }
        let present = |d: &PredictorData, r: usize| match d {
            PredictorData::Continuous(v) => v[r].is_finite(),
            PredictorData::Categorical(v) => !v[r].trim().is_empty(),
        };
        let mut categories = Vec::new();
        let (y_all, coding) = if let Coding::Categorical(order) = coding {
            let labels: Vec<String> = (0..table.n_rows()).map(|r| rc.level_text(r)).collect();
            let mut seen: Vec<String> = (0..table.n_rows())
                .filter(|&r| !states.is_excluded(r) && data.iter().all(|d| present(d, r)))
                .map(|r| labels[r].clone())
                .filter(|l| !l.trim().is_empty())
                .collect();
            seen.sort_by(|a, b| natural_order(a, b));
            seen.dedup();
            categories = order.iter().filter(|l| seen.contains(l)).cloned().collect();
            for l in seen {
                if !categories.contains(&l) {
                    categories.push(l);
                }
            }
            let y =
                labels.iter().map(|l| categories.iter().position(|c| c == l).map_or(f64::NAN, |k| k as f64)).collect();
            (y, None)
        } else if let Coding::Binary(event) = coding {
            // The two levels come from the rows a fit can use (included, with
            // every predictor), so a stray label in an excluded or incomplete
            // row does not block the fit; such labels are missing.
            let labels: Vec<String> = (0..table.n_rows()).map(|r| rc.level_text(r)).collect();
            let mut levels: Vec<String> = (0..table.n_rows())
                .filter(|&r| !states.is_excluded(r) && data.iter().all(|d| present(d, r)))
                .map(|r| labels[r].clone())
                .filter(|l| !l.trim().is_empty())
                .collect();
            levels.sort_by(|a, b| natural_order(a, b));
            levels.dedup();
            if levels.len() != 2 {
                return Err(invalid(format!(
                    "a binomial response needs exactly two distinct values; {:?} has {}",
                    rc.name,
                    levels.len()
                )));
            }
            if let Some(event) = event {
                match levels.iter().position(|l| l == event) {
                    Some(0) => levels.swap(0, 1),
                    Some(_) => {}
                    None => {
                        return Err(invalid(format!(
                            "the event {event:?} is not one of the values of {:?} ({} and {})",
                            rc.name, levels[0], levels[1]
                        )));
                    }
                }
            }
            let y = labels
                .iter()
                .map(|l| {
                    if *l == levels[1] {
                        1.0
                    } else if *l == levels[0] {
                        0.0
                    } else {
                        f64::NAN
                    }
                })
                .collect();
            (y, Some(BinaryCoding { other: levels[0].clone(), event: levels[1].clone() }))
        } else {
            let v = rc.as_numeric().ok_or_else(|| invalid(format!("the response {:?} must be numeric", rc.name)))?;
            (v.to_vec(), None)
        };
        let complete: Vec<usize> =
            (0..table.n_rows()).filter(|&r| y_all[r].is_finite() && data.iter().all(|d| present(d, r))).collect();
        let rows_used: Vec<usize> = complete.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        Ok(Self {
            response_name: rc.name.clone(),
            n_missing: table.n_rows() - complete.len(),
            n_excluded: complete.len() - rows_used.len(),
            y_all,
            binary: coding,
            categories,
            predictor_names: names,
            predictors: data,
            factors: crate::prediction_formula::factor_columns(table, predictors),
            rows_used,
        })
    }

    pub fn borrowed(&self) -> Vec<Predictor<'_>> {
        self.predictor_names
            .iter()
            .zip(&self.predictors)
            .map(|(name, d)| Predictor {
                name,
                values: match d {
                    PredictorData::Continuous(v) => PredictorValues::Continuous(v),
                    PredictorData::Categorical(v) => PredictorValues::Categorical(v),
                },
            })
            .collect()
    }

    /// The design for the rows used, its matrix, and the response.
    pub fn design(&self) -> Result<(ModelDesign, Matrix, Vec<f64>)> {
        let preds = self.borrowed();
        let design = ModelDesign::new(&preds, &self.rows_used)?;
        let x = design.matrix(&preds, &self.rows_used)?;
        let y = self.rows_used.iter().map(|&r| self.y_all[r]).collect();
        Ok((design, x, y))
    }

    /// The design with `effects` (positions in the predictors) and spline
    /// terms for the rows used, its matrix, and the response.
    pub fn design_with(
        &self,
        effects: &[Vec<(usize, u8)>],
        splines: &[Option<stat_linear::SplineSpec>],
    ) -> Result<(ModelDesign, Matrix, Vec<f64>)> {
        let preds = self.borrowed();
        let design = ModelDesign::with_splines(&preds, &self.rows_used, effects, splines)?;
        let x = design.matrix(&preds, &self.rows_used)?;
        let y = self.rows_used.iter().map(|&r| self.y_all[r]).collect();
        Ok((design, x, y))
    }

    /// `predict` applied to every table row with all predictors present at
    /// levels `design` knows; NaN for the other rows.
    ///
    /// # Errors
    /// Whatever `predict` returns.
    pub fn predict_all(&self, design: &ModelDesign, predict: impl Fn(&Matrix) -> Result<Vec<f64>>) -> Result<Vec<f64>> {
        let preds = self.borrowed();
        let encoded: Vec<Option<Vec<f64>>> = (0..self.y_all.len())
            .map(|r| design.row(&preds, r).filter(|row| row.iter().all(|v| v.is_finite())))
            .collect();
        let known: Vec<Vec<f64>> = encoded.iter().flatten().cloned().collect();
        if known.is_empty() {
            return Ok(vec![f64::NAN; encoded.len()]);
        }
        let mut predicted = predict(&Matrix::from_rows(&known)?)?.into_iter();
        Ok(encoded.iter().map(|e| if e.is_some() { predicted.next().unwrap_or(f64::NAN) } else { f64::NAN }).collect())
    }

    /// Profiler factors of `design` (one per predictor): continuous ranges
    /// and categorical levels over the rows used.
    pub fn profile_factors(&self, design: &ModelDesign) -> Vec<crate::surface_profiler::ProfileFactor> {
        use crate::surface_profiler::{ProfileFactor, ProfileFactorKind};
        design
            .terms()
            .iter()
            .zip(&self.predictors)
            .map(|(term, data)| ProfileFactor {
                name: term.name.clone(),
                kind: match (&term.kind, data) {
                    (TermKind::Categorical { levels }, _) => ProfileFactorKind::Categorical { levels: levels.clone() },
                    (_, PredictorData::Continuous(v)) => {
                        let used = self.rows_used.iter().map(|&r| v[r]);
                        let lo = used.clone().fold(f64::INFINITY, f64::min);
                        let hi = used.fold(f64::NEG_INFINITY, f64::max);
                        ProfileFactorKind::Continuous { range: if lo <= hi { (lo, hi) } else { (0.0, 1.0) } }
                    }
                    (_, PredictorData::Categorical(_)) => ProfileFactorKind::Continuous { range: (0.0, 1.0) },
                },
            })
            .collect()
    }

    /// Starting profiler settings: continuous predictors at their mean and
    /// categorical ones at their most frequent level, over the rows used.
    pub fn default_settings(&self, design: &ModelDesign) -> Vec<TermValue> {
        design
            .terms()
            .iter()
            .zip(&self.predictors)
            .map(|(term, data)| match (&term.kind, data) {
                (TermKind::Categorical { levels }, PredictorData::Categorical(labels)) => {
                    let counts: Vec<usize> =
                        levels.iter().map(|l| self.rows_used.iter().filter(|&&r| labels[r] == *l).count()).collect();
                    let best = counts.iter().copied().max().unwrap_or(0);
                    TermValue::Level(counts.iter().position(|&c| c == best).unwrap_or(0))
                }
                (TermKind::Categorical { .. }, _) => TermValue::Level(0),
                (_, PredictorData::Continuous(v)) => {
                    let used: Vec<f64> = self.rows_used.iter().map(|&r| v[r]).collect();
                    TermValue::Number(stat_core::summation::mean(&used).unwrap_or(0.0))
                }
                (_, PredictorData::Categorical(_)) => TermValue::Number(0.0),
            })
            .collect()
    }

    /// The rows used as profiler observations under `design`.
    pub fn observations(&self, design: &ModelDesign) -> Vec<crate::surface_profiler::Observation> {
        self.rows_used
            .iter()
            .map(|&r| crate::surface_profiler::Observation {
                row: r,
                settings: design
                    .terms()
                    .iter()
                    .zip(&self.predictors)
                    .map(|(term, data)| match (&term.kind, data) {
                        (TermKind::Categorical { levels }, PredictorData::Categorical(labels)) => {
                            TermValue::Level(levels.iter().position(|l| *l == labels[r]).unwrap_or(0))
                        }
                        (_, PredictorData::Continuous(v)) => TermValue::Number(v[r]),
                        _ => TermValue::Number(f64::NAN),
                    })
                    .collect(),
                response: self.y_all[r],
            })
            .collect()
    }

    /// The single continuous predictor's values for the rows used, if the
    /// model has exactly one predictor and it is continuous.
    pub fn single_continuous(&self) -> Option<Vec<f64>> {
        match self.predictors.as_slice() {
            [PredictorData::Continuous(v)] => Some(self.rows_used.iter().map(|&r| v[r]).collect()),
            _ => None,
        }
    }
}

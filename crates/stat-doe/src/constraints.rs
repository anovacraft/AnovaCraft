//! Constrained design regions for optimal designs.
//!
//! A [`Region`] is the coded cube `[−1, 1]^k` cut by
//!
//! * **linear inequality constraints** `Σⱼ aⱼ xⱼ ≤ b` (a `≥` constraint is
//!   stored with its signs reversed), for example a combined limit on two
//!   settings that cannot both be high, and
//! * **disallowed combinations**, each a conjunction of per-factor
//!   conditions (`<`, `≤`, `>`, `≥`, `=`): a point is excluded when it meets
//!   every condition of one combination, for example "temperature above 180
//!   together with time above 25".
//!
//! A factor can also be restricted to its two coded settings −1 and +1 (a
//! two-level categorical factor), see [`Region::restrict_to_two_levels`].
//!
//! [`Region::parse`] reads constraints written in the factors' actual units,
//! one per line:
//!
//! ```text
//! 2*Temp + Time <= 400
//! Temp - Time >= 10
//! exclude Temp > 180 and Time > 25
//! ```
//!
//! and converts them to coded units through each factor's low and high
//! settings: `x_actual = m + h·x_coded` with `m = (low + high)/2` and
//! `h = (high − low)/2`, so `Σ aⱼ x_actual,ⱼ ≤ b` becomes
//! `Σ (aⱼ hⱼ) xⱼ ≤ b − Σ aⱼ mⱼ`.
//!
//! The coordinate-exchange search ([`crate::optimal`]) asks the region for the
//! **feasible set of one coordinate** given the others
//! ([`Region::feasible_set`]): the interval left by the cube and the linear
//! constraints, minus the values that would complete a disallowed
//! combination. Points on a constraint boundary are allowed; a point exactly
//! on the boundary of a strict condition (`<`, `>`) in a disallowed
//! combination is allowed, one on a non-strict condition (`≤`, `≥`, `=`) is
//! excluded. Comparisons allow a tolerance of 10⁻⁹ in coded units in favour
//! of the allowed region.
//!
//! Following the constrained coordinate-exchange idea of Meyer & Nachtsheim
//! (1995, §5), each run can also slide along an active linear constraint
//! ([`Region::slide_directions`]), which a coordinate move alone cannot do
//! (on a slanted boundary every coordinate move leaves it).

use crate::design::{Factor, Rng};
use stat_core::{Result, StatError};

/// Tolerance of the comparisons, in coded units.
pub const TOLERANCE: f64 = 1e-9;

/// A comparison in a condition of a disallowed combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// `<`
    Less,
    /// `≤`
    AtMost,
    /// `>`
    Greater,
    /// `≥`
    AtLeast,
    /// `=`
    Equal,
}

impl Comparison {
    /// The comparison with its sides swapped (`a < b` ⇔ `b > a`).
    #[must_use]
    pub fn flipped(self) -> Self {
        match self {
            Self::Less => Self::Greater,
            Self::AtMost => Self::AtLeast,
            Self::Greater => Self::Less,
            Self::AtLeast => Self::AtMost,
            Self::Equal => Self::Equal,
        }
    }

    /// Symbol for descriptions.
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::AtMost => "≤",
            Self::Greater => ">",
            Self::AtLeast => "≥",
            Self::Equal => "=",
        }
    }
}

/// `Σⱼ coefficients[j] · xⱼ ≤ bound` in coded units.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearConstraint {
    /// One coefficient per factor.
    pub coefficients: Vec<f64>,
    /// Right-hand side.
    pub bound: f64,
}

impl LinearConstraint {
    /// `bound − Σ aⱼ xⱼ` (non-negative inside).
    #[must_use]
    pub fn slack(&self, x: &[f64]) -> f64 {
        self.bound - self.coefficients.iter().zip(x).map(|(a, v)| a * v).sum::<f64>()
    }
}

/// One condition of a disallowed combination, in coded units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    /// Factor index.
    pub factor: usize,
    /// Comparison.
    pub comparison: Comparison,
    /// Coded value compared with.
    pub value: f64,
}

impl Condition {
    /// Whether coded value `x` meets the condition (boundary cases resolved
    /// towards the allowed region, see the module documentation).
    #[must_use]
    pub fn holds(&self, x: f64) -> bool {
        let v = self.value;
        match self.comparison {
            Comparison::Less => x < v - TOLERANCE,
            Comparison::AtMost => x <= v + TOLERANCE,
            Comparison::Greater => x > v + TOLERANCE,
            Comparison::AtLeast => x >= v - TOLERANCE,
            Comparison::Equal => (x - v).abs() <= TOLERANCE,
        }
    }
}

/// A disallowed combination: points meeting every condition are excluded.
#[derive(Debug, Clone, PartialEq)]
pub struct Exclusion {
    /// The conditions, all of which must hold for a point to be excluded.
    pub conditions: Vec<Condition>,
}

impl Exclusion {
    /// Whether `x` falls in this disallowed combination.
    #[must_use]
    pub fn excludes(&self, x: &[f64]) -> bool {
        self.conditions.iter().all(|c| c.holds(x[c.factor]))
    }
}

/// An interval of one coordinate, with open or closed ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    /// Lower end.
    pub lo: f64,
    /// Upper end.
    pub hi: f64,
    /// Whether `lo` belongs to the interval.
    pub lo_closed: bool,
    /// Whether `hi` belongs to the interval.
    pub hi_closed: bool,
}

impl Interval {
    fn closed(lo: f64, hi: f64) -> Self {
        Self { lo, hi, lo_closed: true, hi_closed: true }
    }

    fn nonempty(&self) -> bool {
        self.lo < self.hi || (self.lo == self.hi && self.lo_closed && self.hi_closed)
    }

    /// Length `hi − lo`.
    #[must_use]
    pub fn length(&self) -> f64 {
        self.hi - self.lo
    }

    /// Whether `v` lies in the interval.
    #[must_use]
    pub fn contains(&self, v: f64) -> bool {
        (v > self.lo || (v == self.lo && self.lo_closed)) && (v < self.hi || (v == self.hi && self.hi_closed))
    }

    /// The parts of `self` outside `e`.
    fn subtract(&self, e: &Self) -> Vec<Self> {
        let mut out = Vec::new();
        // Left of e.
        let (a, ac) = if self.hi < e.lo {
            (self.hi, self.hi_closed)
        } else if self.hi == e.lo {
            (self.hi, self.hi_closed && !e.lo_closed)
        } else {
            (e.lo, !e.lo_closed)
        };
        let left = Self { lo: self.lo, hi: a, lo_closed: self.lo_closed, hi_closed: ac };
        if left.nonempty() {
            out.push(left);
        }
        // Right of e.
        let (b, bc) = if self.lo > e.hi {
            (self.lo, self.lo_closed)
        } else if self.lo == e.hi {
            (self.lo, self.lo_closed && !e.hi_closed)
        } else {
            (e.hi, !e.hi_closed)
        };
        let right = Self { lo: b, hi: self.hi, lo_closed: bc, hi_closed: self.hi_closed };
        if right.nonempty() {
            out.push(right);
        }
        out
    }
}

/// A direction to slide a run along an active linear constraint: the run
/// may move to `x + t·direction` for `t` in `[t_min, t_max]` without leaving
/// the cube or breaking a linear constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// Direction (two non-zero entries).
    pub direction: Vec<f64>,
    /// Most negative step.
    pub t_min: f64,
    /// Most positive step.
    pub t_max: f64,
}

/// The coded cube `[−1, 1]^k` cut by linear constraints and disallowed
/// combinations (see the module documentation).
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    k: usize,
    linear: Vec<LinearConstraint>,
    exclusions: Vec<Exclusion>,
    two_level: Vec<bool>,
    descriptions: Vec<String>,
}

impl Region {
    /// The whole cube `[−1, 1]^k`.
    #[must_use]
    pub fn cube(k: usize) -> Self {
        Self { k, linear: Vec::new(), exclusions: Vec::new(), two_level: vec![false; k], descriptions: Vec::new() }
    }

    /// Number of factors.
    #[must_use]
    pub fn n_factors(&self) -> usize {
        self.k
    }

    /// `true` for the whole cube with every factor continuous.
    #[must_use]
    pub fn is_unconstrained(&self) -> bool {
        self.linear.is_empty() && self.exclusions.is_empty() && !self.two_level.iter().any(|&t| t)
    }

    /// The linear constraints, in coded units.
    #[must_use]
    pub fn linear(&self) -> &[LinearConstraint] {
        &self.linear
    }

    /// The disallowed combinations, in coded units.
    #[must_use]
    pub fn exclusions(&self) -> &[Exclusion] {
        &self.exclusions
    }

    /// Which factors may take only the coded settings −1 and +1.
    #[must_use]
    pub fn two_level_factors(&self) -> &[bool] {
        &self.two_level
    }

    /// One line per constraint, as written (for design notes).
    #[must_use]
    pub fn describe(&self) -> &[String] {
        &self.descriptions
    }

    /// Add `Σ aⱼ xⱼ ≤ b` in coded units.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a wrong number of coefficients, all
    /// zero, or values that are not finite.
    pub fn add_linear(&mut self, coefficients: Vec<f64>, bound: f64, description: impl Into<String>) -> Result<()> {
        if coefficients.len() != self.k {
            return Err(invalid(format!("a constraint needs {} coefficients", self.k)));
        }
        if !bound.is_finite() || coefficients.iter().any(|a| !a.is_finite()) {
            return Err(invalid("constraint coefficients must be finite numbers"));
        }
        let scale = coefficients.iter().fold(0.0_f64, |m, a| m.max(a.abs()));
        if scale == 0.0 {
            return Err(invalid("a constraint must involve at least one factor"));
        }
        self.linear.push(LinearConstraint {
            coefficients: coefficients.iter().map(|a| a / scale).collect(),
            bound: bound / scale,
        });
        self.descriptions.push(description.into());
        Ok(())
    }

    /// Add a disallowed combination (coded units).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for no conditions, an unknown factor or
    /// a value that is not finite.
    pub fn add_exclusion(&mut self, conditions: Vec<Condition>, description: impl Into<String>) -> Result<()> {
        if conditions.is_empty() {
            return Err(invalid("a disallowed combination needs at least one condition"));
        }
        if conditions.iter().any(|c| c.factor >= self.k || !c.value.is_finite()) {
            return Err(invalid("a condition names an unknown factor or a value that is not finite"));
        }
        self.exclusions.push(Exclusion { conditions });
        self.descriptions.push(description.into());
        Ok(())
    }

    /// Restrict factor `j` to the coded settings −1 and +1 (for example a
    /// two-level categorical factor).
    pub fn restrict_to_two_levels(&mut self, j: usize) {
        if let Some(t) = self.two_level.get_mut(j) {
            *t = true;
        }
    }

    /// Read constraints in actual units, one per line (blank lines and lines
    /// starting with `#` are skipped):
    ///
    /// * a linear constraint `expression ≤ expression` (or `<`, `>=`, `≥`,
    ///   `>`), where each side is a sum of terms `number`, `factor`,
    ///   `number*factor` or `factor/number`, for example
    ///   `2*Temp + Time <= 400` or `Temp - Time >= 10`; strict and non-strict
    ///   comparisons are treated alike (the factors are continuous);
    /// * a disallowed combination `exclude condition and condition …`, each
    ///   condition comparing one factor with a value, for example
    ///   `exclude Temp > 180 and Time > 25` (also `&` between conditions).
    ///
    /// Factor names are matched without regard to case (the longest name
    /// that fits wins, so names may contain spaces); a name can also be
    /// written in double quotes.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] naming the line for an unknown name, a
    /// non-linear term (a product or quotient of two factors), a missing or
    /// repeated comparison, an equality constraint, or a condition that does
    /// not name exactly one factor.
    pub fn parse(text: &str, factors: &[Factor]) -> Result<Self> {
        let mut region = Self::cube(factors.len());
        for (line_no, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let fail = |m: String| invalid(format!("constraint line {}: {m}", line_no + 1));
            let tokens = tokenize(line, factors).map_err(fail)?;
            region.add_parsed(&tokens, line, factors).map_err(|e| match e {
                StatError::InvalidArgument { message } => fail(message),
                other => other,
            })?;
        }
        Ok(region)
    }

    fn add_parsed(&mut self, tokens: &[Token], line: &str, factors: &[Factor]) -> Result<()> {
        let k = factors.len();
        let mid: Vec<f64> = factors.iter().map(|f| (f.low + f.high) / 2.0).collect();
        let half: Vec<f64> = factors.iter().map(|f| (f.high - f.low) / 2.0).collect();
        if half.iter().any(|h| *h == 0.0 || !h.is_finite()) {
            return Err(invalid("every factor needs different, finite low and high settings"));
        }
        if matches!(tokens.first(), Some(Token::Word(w)) if w.eq_ignore_ascii_case("exclude")) {
            let mut conditions = Vec::new();
            for part in tokens[1..].split(|t| matches!(t, Token::And)) {
                if part.is_empty() {
                    return Err(invalid("expected a condition such as Temp > 180 after \"exclude\" or \"and\""));
                }
                let (lhs, cmp, rhs) = split_comparison(part)?;
                let (coef, constant) = difference(&parse_linear(lhs, k)?, &parse_linear(rhs, k)?);
                let named: Vec<usize> = (0..k).filter(|&j| coef[j] != 0.0).collect();
                let [j] = named[..] else {
                    return Err(invalid("each condition of an exclusion compares one factor with a value"));
                };
                // coef·x + constant (cmp) 0  ⇔  x (cmp') −constant/coef.
                let mut cmp = if coef[j] < 0.0 { cmp.flipped() } else { cmp };
                let actual = -constant / coef[j];
                if half[j] < 0.0 {
                    cmp = cmp.flipped();
                }
                conditions.push(Condition { factor: j, comparison: cmp, value: (actual - mid[j]) / half[j] });
            }
            let shown = line[line.find(char::is_whitespace).unwrap_or(line.len())..].trim();
            return self.add_exclusion(conditions, format!("Exclude {shown}"));
        }
        let (lhs, cmp, rhs) = split_comparison(tokens)?;
        let (coef, constant) = match cmp {
            Comparison::Less | Comparison::AtMost => difference(&parse_linear(lhs, k)?, &parse_linear(rhs, k)?),
            Comparison::Greater | Comparison::AtLeast => difference(&parse_linear(rhs, k)?, &parse_linear(lhs, k)?),
            Comparison::Equal => {
                return Err(invalid(
                    "equality constraints leave no room for an experiment; use ≤ or ≥ (or exclude … for a combination)",
                ));
            }
        };
        if coef.iter().all(|a| *a == 0.0) {
            return Err(invalid("the constraint does not involve any factor"));
        }
        // Σ aⱼ (mⱼ + hⱼ zⱼ) + c ≤ 0.
        let coded: Vec<f64> = coef.iter().zip(&half).map(|(a, h)| a * h).collect();
        let bound = -constant - coef.iter().zip(&mid).map(|(a, m)| a * m).sum::<f64>();
        self.add_linear(coded, bound, line.to_owned())
    }

    /// Whether coded point `x` is in the region.
    #[must_use]
    pub fn contains(&self, x: &[f64]) -> bool {
        x.len() == self.k
            && x.iter().all(|v| v.abs() <= 1.0 + TOLERANCE)
            && x.iter().zip(&self.two_level).all(|(v, &t)| !t || (v.abs() - 1.0).abs() <= TOLERANCE)
            && self.linear.iter().all(|c| c.slack(x) >= -TOLERANCE)
            && !self.exclusions.iter().any(|e| e.excludes(x))
    }

    /// The interval of coordinate `j` left by the cube and the linear
    /// constraints with the other coordinates of `x` fixed, or `None` if
    /// empty.
    #[must_use]
    pub fn linear_interval(&self, x: &[f64], j: usize) -> Option<(f64, f64)> {
        let (mut lo, mut hi) = (-1.0_f64, 1.0_f64);
        for c in &self.linear {
            let a = c.coefficients[j];
            let rest = c.bound
                - c.coefficients
                    .iter()
                    .zip(x)
                    .enumerate()
                    .filter(|(i, _)| *i != j)
                    .map(|(_, (a, v))| a * v)
                    .sum::<f64>();
            if a.abs() <= 1e-14 {
                if rest < -TOLERANCE {
                    return None;
                }
            } else if a > 0.0 {
                hi = hi.min(rest / a);
            } else {
                lo = lo.max(rest / a);
            }
        }
        if lo > hi + TOLERANCE {
            return None;
        }
        if lo > hi {
            let m = (lo + hi) / 2.0;
            (lo, hi) = (m, m);
        }
        Some((lo, hi))
    }

    /// The values coordinate `j` may take with the other coordinates of `x`
    /// fixed: the linear interval minus the values that complete a
    /// disallowed combination (and only ±1 for a two-level factor).
    #[must_use]
    pub fn feasible_set(&self, x: &[f64], j: usize) -> Vec<Interval> {
        let Some((lo, hi)) = self.linear_interval(x, j) else { return Vec::new() };
        let mut set = vec![Interval::closed(lo, hi)];
        for e in &self.exclusions {
            if e.conditions.iter().any(|c| c.factor != j && !c.holds(x[c.factor])) {
                continue;
            }
            let mut ex = Interval { lo: f64::NEG_INFINITY, hi: f64::INFINITY, lo_closed: false, hi_closed: false };
            for c in e.conditions.iter().filter(|c| c.factor == j) {
                let v = c.value;
                match c.comparison {
                    Comparison::Less | Comparison::AtMost => {
                        let closed = c.comparison == Comparison::AtMost;
                        if v < ex.hi || (v == ex.hi && !closed) {
                            ex.hi = v;
                            ex.hi_closed = closed;
                        }
                    }
                    Comparison::Greater | Comparison::AtLeast => {
                        let closed = c.comparison == Comparison::AtLeast;
                        if v > ex.lo || (v == ex.lo && !closed) {
                            ex.lo = v;
                            ex.lo_closed = closed;
                        }
                    }
                    Comparison::Equal => {
                        if ex.contains(v) {
                            ex = Interval::closed(v, v);
                        } else {
                            ex = Interval { lo: 1.0, hi: 0.0, lo_closed: false, hi_closed: false };
                        }
                    }
                }
            }
            if ex.nonempty() {
                set = set.iter().flat_map(|i| i.subtract(&ex)).collect();
            }
        }
        if self.two_level[j] {
            set = [-1.0, 1.0]
                .into_iter()
                .filter(|v| set.iter().any(|i| i.contains(*v)))
                .map(|v| Interval::closed(v, v))
                .collect();
        }
        set
    }

    /// Candidate values of coordinate `j` for coordinate exchange: the grid
    /// `levels` that are feasible, the closed ends of each feasible piece,
    /// and (when `midpoints`) the middle of each piece; only values that
    /// keep `x` in the region.
    #[must_use]
    pub fn candidates(&self, x: &[f64], j: usize, levels: &[f64], midpoints: bool) -> Vec<f64> {
        let set = self.feasible_set(x, j);
        let mut out: Vec<f64> = Vec::new();
        let mut push = |v: f64| {
            let v = v.clamp(-1.0, 1.0);
            if !out.iter().any(|u| (u - v).abs() <= 1e-12) {
                out.push(v);
            }
        };
        for &v in levels {
            if set.iter().any(|i| i.contains(v)) {
                push(v);
            }
        }
        for i in &set {
            if i.lo_closed {
                push(i.lo);
            }
            if i.hi_closed {
                push(i.hi);
            }
            if midpoints && i.length() > 0.0 {
                push((i.lo + i.hi) / 2.0);
            }
        }
        let mut point = x.to_vec();
        out.retain(|&v| {
            point[j] = v;
            self.contains(&point)
        });
        out
    }

    /// Directions along which run `x` can slide while staying on an active
    /// linear constraint (one with zero slack): for each such constraint
    /// and each pair of continuous factors `j`, `l` it involves, the
    /// direction `e_j − (a_j / a_l) e_l`, with the step range that keeps the
    /// cube and every linear constraint.
    #[must_use]
    pub fn slide_directions(&self, x: &[f64]) -> Vec<Slide> {
        let mut out = Vec::new();
        for c in &self.linear {
            if c.slack(x).abs() > 1e-7 {
                continue;
            }
            let a = &c.coefficients;
            for j in 0..self.k {
                for l in j + 1..self.k {
                    if a[j] == 0.0 || a[l] == 0.0 || self.two_level[j] || self.two_level[l] {
                        continue;
                    }
                    let mut d = vec![0.0; self.k];
                    d[j] = 1.0;
                    d[l] = -a[j] / a[l];
                    let (mut t_min, mut t_max) = (f64::NEG_INFINITY, f64::INFINITY);
                    for (i, &di) in d.iter().enumerate() {
                        if di != 0.0 {
                            let (p, q) = ((-1.0 - x[i]) / di, (1.0 - x[i]) / di);
                            t_min = t_min.max(p.min(q));
                            t_max = t_max.min(p.max(q));
                        }
                    }
                    for other in &self.linear {
                        let ad: f64 = other.coefficients.iter().zip(&d).map(|(u, v)| u * v).sum();
                        if ad.abs() > 1e-12 {
                            let t = other.slack(x).max(0.0) / ad;
                            if ad > 0.0 {
                                t_max = t_max.min(t);
                            } else {
                                t_min = t_min.max(t);
                            }
                        }
                    }
                    if t_max - t_min > 1e-9 {
                        out.push(Slide { direction: d, t_min: t_min.min(0.0), t_max: t_max.max(0.0) });
                    }
                }
            }
        }
        out
    }

    /// A random point of the region: up to 200 uniform draws from the cube
    /// (rejection sampling), then up to 200 points built coordinate by
    /// coordinate inside the range each linear constraint leaves (given the
    /// coordinates already set and the extremes of those not yet set), each
    /// followed by a few sweeps that redraw every coordinate from its
    /// feasible set.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if no point was found: the region is
    /// empty or too small to sample.
    pub fn random_point(&self, rng: &mut Rng) -> Result<Vec<f64>> {
        let draw_two = |rng: &mut Rng| if rng.next_f64() < 0.5 { -1.0 } else { 1.0 };
        for _ in 0..200 {
            let x: Vec<f64> =
                self.two_level.iter().map(|&t| if t { draw_two(rng) } else { 2.0 * rng.next_f64() - 1.0 }).collect();
            if self.contains(&x) {
                return Ok(x);
            }
        }
        for _ in 0..200 {
            let mut order: Vec<usize> = (0..self.k).collect();
            rng.shuffle(&mut order);
            let mut x = vec![0.0; self.k];
            let mut set = vec![false; self.k];
            for &j in &order {
                let (mut lo, mut hi) = (-1.0_f64, 1.0_f64);
                for c in &self.linear {
                    let a = c.coefficients[j];
                    let rest = c.bound
                        - (0..self.k)
                            .filter(|&i| i != j)
                            .map(|i| if set[i] { c.coefficients[i] * x[i] } else { -c.coefficients[i].abs() })
                            .sum::<f64>();
                    if a > 1e-14 {
                        hi = hi.min(rest / a);
                    } else if a < -1e-14 {
                        lo = lo.max(rest / a);
                    }
                }
                x[j] = if self.two_level[j] {
                    draw_two(rng)
                } else if lo <= hi {
                    lo + (hi - lo) * rng.next_f64()
                } else {
                    (lo + hi).clamp(-2.0, 2.0) / 2.0
                };
                set[j] = true;
            }
            for _ in 0..5 {
                if self.contains(&x) {
                    return Ok(x);
                }
                for j in 0..self.k {
                    if let Some(v) = draw_from(&self.feasible_set(&x, j), rng) {
                        x[j] = v;
                    }
                }
            }
            if self.contains(&x) {
                return Ok(x);
            }
        }
        Err(invalid("the constraints leave no room (or too little room) to place runs: check them for conflicts"))
    }

    /// `count` points of the region from the Halton sequence (bases the
    /// first `k` primes; a two-level factor takes −1 below ½ and +1 above),
    /// keeping those in the region, from at most `200 · count` draws. Used
    /// to estimate moments of the region.
    #[must_use]
    pub fn quasi_random_points(&self, count: usize) -> Vec<Vec<f64>> {
        let bases = first_primes(self.k);
        let mut out = Vec::with_capacity(count);
        let mut index: u64 = 1;
        let limit = (count as u64).saturating_mul(200);
        while out.len() < count && index <= limit {
            let x: Vec<f64> = bases
                .iter()
                .zip(&self.two_level)
                .map(|(&b, &t)| {
                    let u = radical_inverse(index, b);
                    if t { if u < 0.5 { -1.0 } else { 1.0 } } else { 2.0 * u - 1.0 }
                })
                .collect();
            if self.contains(&x) {
                out.push(x);
            }
            index += 1;
        }
        out
    }
}

/// A uniform draw from a union of intervals (degenerate pieces picked at
/// random when the union has no length).
fn draw_from(set: &[Interval], rng: &mut Rng) -> Option<f64> {
    if set.is_empty() {
        return None;
    }
    let total: f64 = set.iter().map(Interval::length).sum();
    if total <= 0.0 {
        return Some(set[rng.below(set.len())].lo);
    }
    let mut u = rng.next_f64() * total;
    for i in set {
        if u <= i.length() && i.length() > 0.0 {
            return Some(i.lo + u);
        }
        u -= i.length();
    }
    set.last().map(|i| i.hi)
}

fn first_primes(n: usize) -> Vec<u64> {
    let mut primes: Vec<u64> = Vec::with_capacity(n);
    let mut c = 2u64;
    while primes.len() < n {
        if primes.iter().all(|p| c % p != 0) {
            primes.push(c);
        }
        c += 1;
    }
    primes
}

fn radical_inverse(mut i: u64, base: u64) -> f64 {
    let (mut f, mut r) = (1.0, 0.0);
    while i > 0 {
        f /= base as f64;
        r += f * (i % base) as f64;
        i /= base;
    }
    r
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Factor(usize),
    Word(String),
    Op(char),
    Cmp(Comparison),
    And,
}

fn tokenize(line: &str, factors: &[Factor]) -> std::result::Result<Vec<Token>, String> {
    let chars: Vec<char> = line.chars().collect();
    let lower: Vec<char> = line.to_lowercase().chars().collect();
    let names: Vec<Vec<char>> = factors.iter().map(|f| f.name.trim().to_lowercase().chars().collect()).collect();
    let word_char = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    let mut i = 0;
    // Lower-casing can change the length of rare characters; then fall back
    // to exact matching.
    let lower_ok = lower.len() == chars.len();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            out.push(Token::Number(text.parse().map_err(|_| format!("{text:?} is not a number"))?));
            continue;
        }
        if c == '"' {
            let end = chars[i + 1..].iter().position(|&d| d == '"').ok_or("a quoted name is not closed")?;
            let name: String = chars[i + 1..i + 1 + end].iter().collect();
            let j = factors
                .iter()
                .position(|f| f.name.trim().eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| format!("unknown factor {name:?}"))?;
            out.push(Token::Factor(j));
            i += end + 2;
            continue;
        }
        // Longest factor name starting here, ending at a word boundary.
        let source = if lower_ok { &lower } else { &chars };
        let best = names
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                !n.is_empty()
                    && source.len() >= i + n.len()
                    && source[i..i + n.len()] == n[..]
                    && !(n.last().is_some_and(|&l| word_char(l))
                        && chars.get(i + n.len()).is_some_and(|&d| word_char(d)))
            })
            .max_by_key(|(_, n)| n.len());
        if let Some((j, n)) = best {
            out.push(Token::Factor(j));
            i += n.len();
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let (token, len) = match (c, two.as_str()) {
            (_, "<=") => (Token::Cmp(Comparison::AtMost), 2),
            (_, ">=") => (Token::Cmp(Comparison::AtLeast), 2),
            (_, "==") => (Token::Cmp(Comparison::Equal), 2),
            (_, "&&") => (Token::And, 2),
            ('≤', _) => (Token::Cmp(Comparison::AtMost), 1),
            ('≥', _) => (Token::Cmp(Comparison::AtLeast), 1),
            ('<', _) => (Token::Cmp(Comparison::Less), 1),
            ('>', _) => (Token::Cmp(Comparison::Greater), 1),
            ('=', _) => (Token::Cmp(Comparison::Equal), 1),
            ('&', _) => (Token::And, 1),
            ('+' | '*' | '/', _) => (Token::Op(c), 1),
            ('-' | '−', _) => (Token::Op('-'), 1),
            ('×' | '·', _) => (Token::Op('*'), 1),
            _ if word_char(c) => {
                let start = i;
                let mut j = i;
                while j < chars.len() && word_char(chars[j]) {
                    j += 1;
                }
                let word: String = chars[start..j].iter().collect();
                if word.eq_ignore_ascii_case("and") {
                    (Token::And, j - i)
                } else if word.eq_ignore_ascii_case("exclude") && out.is_empty() {
                    (Token::Word(word), j - i)
                } else {
                    return Err(format!("unknown factor {word:?}"));
                }
            }
            _ => return Err(format!("unexpected character {c:?}")),
        };
        out.push(token);
        i += len;
    }
    Ok(out)
}

/// Split at the single comparison.
fn split_comparison(tokens: &[Token]) -> Result<(&[Token], Comparison, &[Token])> {
    let positions: Vec<usize> =
        tokens.iter().enumerate().filter(|(_, t)| matches!(t, Token::Cmp(_))).map(|(i, _)| i).collect();
    match positions[..] {
        [p] => {
            let Token::Cmp(cmp) = tokens[p] else { unreachable!("filtered on comparisons") };
            if p == 0 || p + 1 == tokens.len() {
                return Err(invalid("a comparison needs something on both sides"));
            }
            Ok((&tokens[..p], cmp, &tokens[p + 1..]))
        }
        [] => Err(invalid("expected a comparison such as <= or >=")),
        _ => Err(invalid("use one comparison per line")),
    }
}

/// A linear expression: (coefficients, constant).
type Linear = (Vec<f64>, f64);

fn difference(a: &Linear, b: &Linear) -> Linear {
    (a.0.iter().zip(&b.0).map(|(x, y)| x - y).collect(), a.1 - b.1)
}

/// Parse `± term ± term …`, each term a product of numbers and at most one
/// factor, optionally divided by numbers.
fn parse_linear(tokens: &[Token], k: usize) -> Result<Linear> {
    let mut coef = vec![0.0; k];
    let mut constant = 0.0;
    let mut i = 0;
    if tokens.is_empty() {
        return Err(invalid("an empty side of a comparison"));
    }
    while i < tokens.len() {
        let mut sign = 1.0;
        let mut any_sign = false;
        while let Some(Token::Op(op @ ('+' | '-'))) = tokens.get(i) {
            if *op == '-' {
                sign = -sign;
            }
            any_sign = true;
            i += 1;
        }
        if i > 0 && !any_sign {
            return Err(invalid("expected + or − between terms"));
        }
        let mut value = sign;
        let mut factor: Option<usize> = None;
        let mut expect_item = true;
        let mut dividing = false;
        loop {
            match tokens.get(i) {
                Some(Token::Number(v)) if expect_item => {
                    if dividing {
                        if *v == 0.0 {
                            return Err(invalid("division by zero"));
                        }
                        value /= v;
                    } else {
                        value *= v;
                    }
                }
                Some(Token::Factor(j)) if expect_item => {
                    if dividing {
                        return Err(invalid("dividing by a factor is not linear"));
                    }
                    if factor.is_some() {
                        return Err(invalid("a product of two factors is not linear"));
                    }
                    factor = Some(*j);
                }
                Some(Token::Op(op @ ('*' | '/'))) if !expect_item => {
                    dividing = *op == '/';
                    expect_item = true;
                    i += 1;
                    continue;
                }
                Some(Token::Op('+' | '-')) | None if !expect_item => break,
                Some(Token::Number(_) | Token::Factor(_)) => return Err(invalid("expected + or − between terms")),
                Some(Token::Word(w)) => return Err(invalid(format!("unexpected word {w:?}"))),
                Some(Token::And) => return Err(invalid("\"and\" joins the conditions of an exclude line")),
                Some(_) | None => return Err(invalid("incomplete expression")),
            }
            expect_item = false;
            dividing = false;
            i += 1;
        }
        match factor {
            Some(j) => coef[j] += value,
            None => constant += value,
        }
    }
    Ok((coef, constant))
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_time() -> Vec<Factor> {
        vec![Factor::new("Temp", 140.0, 200.0), Factor::new("Reaction time", 10.0, 30.0)]
    }

    #[test]
    fn parses_constraints_in_actual_units() {
        let f = temp_time();
        let text = "# limits\n2*Temp + Reaction time <= 400\n\nTemp - reaction TIME >= 140\nexclude Temp > 180 and Reaction time ≥ 25";
        let r = Region::parse(text, &f).unwrap();
        assert_eq!(r.linear().len(), 2);
        assert_eq!(r.exclusions().len(), 1);
        assert_eq!(r.describe()[2], "Exclude Temp > 180 and Reaction time ≥ 25");
        // Temp = 170 + 30 z₁, time = 20 + 10 z₂: 2T + t ≤ 400 ⇔ 60 z₁ + 10 z₂ ≤ 40.
        let c = &r.linear()[0];
        assert!((c.coefficients[0] - 1.0).abs() < 1e-15 && (c.coefficients[1] - 1.0 / 6.0).abs() < 1e-15);
        assert!((c.bound - 40.0 / 60.0).abs() < 1e-15);
        // T − t ≥ 140 ⇔ −30 z₁ + 10 z₂ ≤ 10.
        let c = &r.linear()[1];
        assert!((c.coefficients[0] + 1.0).abs() < 1e-15 && (c.coefficients[1] - 1.0 / 3.0).abs() < 1e-15);
        assert!((c.bound - 1.0 / 3.0).abs() < 1e-15);
        let coded = |t: f64, time: f64| [(t - 170.0) / 30.0, (time - 20.0) / 10.0];
        assert!(r.contains(&coded(160.0, 15.0)));
        assert!(!r.contains(&coded(200.0, 10.0)), "2·200 + 10 > 400");
        assert!(r.contains(&coded(195.0, 10.0)), "on the boundary 2·195 + 10 = 400");
        assert!(!r.contains(&coded(140.0, 30.0)), "140 − 30 < 140");
        assert!(!r.contains(&coded(185.0, 25.0)), "the disallowed combination (≥ 25 includes 25)");
        assert!(r.contains(&coded(180.0, 25.0)), "> 180 excludes 180 itself");
        // Quoted names, unicode comparisons, division and constants on both sides.
        let r = Region::parse("\"Temp\"/2 + 5 ≥ Reaction time - 100", &f).unwrap();
        assert_eq!(r.linear().len(), 1);
        assert!(r.contains(&coded(140.0, 30.0)));
    }

    #[test]
    fn parse_errors_name_the_line() {
        let f = temp_time();
        let err = |t: &str| Region::parse(t, &f).unwrap_err().to_string();
        assert!(
            err("Temp <= 150\nPressure <= 3").contains("line 2: unknown factor \"Pressure\""),
            "{}",
            err("Temp <= 150\nPressure <= 3")
        );
        assert!(err("Temp*Reaction time <= 3").contains("not linear"));
        assert!(err("Temp + Reaction time").contains("comparison"));
        assert!(err("Temp = 150").contains("equality"));
        assert!(err("Temp < 150 < 160").contains("one comparison"));
        assert!(err("exclude Temp + Reaction time > 3").contains("one factor"));
        assert!(err("3 <= 4").contains("does not involve"));
        assert!(err("Temp 2 <= 4").contains("expected + or −"));
        assert!(err("exclude Temp > 3 and").contains("expected a condition"));
        assert!(Region::parse("  \n# nothing\n", &f).unwrap().is_unconstrained());
    }

    #[test]
    fn reversed_factor_settings_flip_conditions() {
        // Low setting above the high one: coded +1 is the smaller value.
        let f = vec![Factor::new("A", 10.0, 0.0)];
        let r = Region::parse("exclude A > 8\nA >= 1", &f).unwrap();
        assert!(!r.contains(&[-1.0]), "A = 10");
        assert!(r.contains(&[-0.6]), "A = 8 (strict)");
        assert!(!r.contains(&[1.0]), "A = 0 < 1");
        assert!(r.contains(&[0.8]), "A = 1");
    }

    #[test]
    fn feasible_sets_and_candidates() {
        let mut r = Region::cube(2);
        r.add_linear(vec![1.0, 1.0], 0.5, "A + B ≤ 0.5").unwrap();
        assert_eq!(r.linear_interval(&[0.0, -1.0], 0), Some((-1.0, 1.0)));
        assert_eq!(r.linear_interval(&[0.0, 0.25], 0), Some((-1.0, 0.25)));
        assert_eq!(r.candidates(&[0.0, 0.25], 0, &[-1.0, 1.0], false), [-1.0, 0.25]);
        assert_eq!(r.candidates(&[0.0, 0.25], 0, &[-1.0, 0.0, 1.0], true), [-1.0, 0.0, 0.25, -0.375]);
        // A disallowed corner: with B = 1, A ≥ 0.5 is excluded.
        r.add_exclusion(
            vec![
                Condition { factor: 0, comparison: Comparison::AtLeast, value: -0.5 },
                Condition { factor: 1, comparison: Comparison::Greater, value: 0.0 },
            ],
            "corner",
        )
        .unwrap();
        let set = r.feasible_set(&[0.0, 1.0], 0);
        assert_eq!(set, [Interval { lo: -1.0, hi: -0.5, lo_closed: true, hi_closed: false }]);
        assert_eq!(r.candidates(&[0.0, 1.0], 0, &[-1.0, 1.0], false), [-1.0], "the open end is not a candidate");
        assert!(r.feasible_set(&[0.0, 2.0], 0).is_empty(), "B = 2 leaves A ≤ −1.5");
        // Slides along the active constraint A + B = 0.5.
        let s = r.slide_directions(&[0.25, 0.25]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].direction, [1.0, -1.0]);
        assert!((s[0].t_min + 0.75).abs() < 1e-12 && (s[0].t_max - 0.75).abs() < 1e-12);
        assert!(r.slide_directions(&[0.0, 0.0]).is_empty(), "inactive");
        // Interval subtraction keeps the open/closed ends right.
        let i = Interval::closed(-1.0, 1.0);
        let e = Interval { lo: 0.0, hi: 0.0, lo_closed: true, hi_closed: true };
        assert_eq!(
            i.subtract(&e),
            [
                Interval { lo: -1.0, hi: 0.0, lo_closed: true, hi_closed: false },
                Interval { lo: 0.0, hi: 1.0, lo_closed: false, hi_closed: true }
            ]
        );
    }

    #[test]
    fn random_points_and_sampling() {
        let mut rng = Rng::new(3);
        // A thin slab: |A + B + C| ≤ 0.01 is hard to hit by rejection.
        let mut r = Region::cube(3);
        r.add_linear(vec![1.0, 1.0, 1.0], 0.01, "upper").unwrap();
        r.add_linear(vec![-1.0, -1.0, -1.0], 0.01, "lower").unwrap();
        for _ in 0..20 {
            let x = r.random_point(&mut rng).unwrap();
            assert!(r.contains(&x));
        }
        // Empty region.
        let mut e = Region::cube(2);
        e.add_linear(vec![1.0, 1.0], -2.5, "A + B ≤ −2.5").unwrap();
        assert!(e.random_point(&mut rng).is_err());
        assert!(e.quasi_random_points(100).is_empty());
        // Two-level factors only take ±1.
        let mut t = Region::cube(2);
        t.restrict_to_two_levels(1);
        let x = t.random_point(&mut rng).unwrap();
        assert!(x[1].abs() == 1.0);
        assert!(!t.contains(&[0.0, 0.0]));
        assert_eq!(t.candidates(&[0.3, 1.0], 1, &[-1.0, 0.0, 1.0], true), [-1.0, 1.0]);
        // Halton points: the first in two dimensions is (½, ⅓) → (0, −⅓).
        let pts = Region::cube(2).quasi_random_points(3);
        assert!((pts[0][0]).abs() < 1e-15 && (pts[0][1] + 1.0 / 3.0).abs() < 1e-15);
        assert_eq!(first_primes(5), [2, 3, 5, 7, 11]);
    }
}

//! The random-effects structure and the penalized least-squares system at the
//! heart of every fit.
//!
//! The model for the linear predictor is `η = offset + Xβ + ZΛ(θ)u` with
//! spherical random effects `u ~ N(0, I)` (times σ² for the normal family),
//! so `b = Λu` has covariance `ΛΛᵀ` (Bates, Mächler, Bolker & Walker,
//! "Fitting linear mixed-effects models using lme4", *J. Stat. Softw.* 67(1),
//! 2015, §3). `Λ` is block diagonal: for random-effects term `k` with `qₖ`
//! effects (intercept, slopes) it repeats one lower-triangular `qₖ × qₖ`
//! factor `Tₖ` for every level of the term's grouping column.
//!
//! Each evaluation solves the penalized weighted least-squares problem
//!
//! ```text
//! min ‖W^½(v − Xβ − ZΛu)‖² + ‖u‖²
//! ```
//!
//! whose normal equations have the block matrix
//! `[ΛᵀZᵀWZΛ + I, ΛᵀZᵀWX; XᵀWZΛ, XᵀWX]`. The term with the most effects
//! (levels × `qₖ`) is eliminated first: its block is itself block diagonal
//! (one small `qₖ × qₖ` block per level), so its Schur complement onto the
//! remaining random effects and β is formed level by level and only that
//! smaller dense matrix is factorised. Its Cholesky factor gives
//! `log|ΛᵀZᵀWZΛ + I|` and `log|R_X|²` (the REML adjustment) directly.

use stat_core::Matrix;

use crate::dense::{Square, backward, cholesky, forward, inverse_from_cholesky, log_det};

/// One random-effects term as the system sees it.
#[derive(Debug, Clone)]
pub(crate) struct Term<'a> {
    /// Effects per level.
    pub q: usize,
    /// Number of levels.
    pub g: usize,
    /// Level of each row.
    pub codes: &'a [usize],
    /// `n × q` random-effects design.
    pub z: &'a Matrix,
    /// Unstructured (correlated) covariance; otherwise diagonal.
    pub correlated: bool,
    /// Where this term's parameters start in θ.
    pub theta_start: usize,
    /// Where this term's effects start in the full u vector.
    pub u_start: usize,
}

impl Term<'_> {
    /// Number of θ parameters.
    pub fn n_theta(&self) -> usize {
        if self.correlated { self.q * (self.q + 1) / 2 } else { self.q }
    }

    /// The lower-triangular factor `T` (row-major `q × q`) from θ: the lower
    /// triangle column by column for a correlated term, the diagonal
    /// otherwise.
    pub fn factor(&self, theta: &[f64]) -> Vec<f64> {
        let q = self.q;
        let t = &theta[self.theta_start..self.theta_start + self.n_theta()];
        let mut out = vec![0.0; q * q];
        if self.correlated {
            let mut k = 0;
            for j in 0..q {
                for i in j..q {
                    out[i * q + j] = t[k];
                    k += 1;
                }
            }
        } else {
            for j in 0..q {
                out[j * q + j] = t[j];
            }
        }
        out
    }

    /// Indices (within this term's θ) of the diagonal entries of `T`.
    pub fn diagonal_indices(&self) -> Vec<usize> {
        if self.correlated {
            let mut idx = Vec::new();
            let mut k = 0;
            for j in 0..self.q {
                idx.push(self.theta_start + k);
                k += self.q - j;
            }
            idx
        } else {
            (0..self.q).map(|j| self.theta_start + j).collect()
        }
    }
}

/// The solution of one penalized least-squares problem.
#[derive(Debug, Clone)]
pub(crate) struct Solution {
    /// Spherical random effects, term by term, level-major.
    pub u: Vec<f64>,
    /// Fixed effects (empty when they were not solved for).
    pub beta: Vec<f64>,
    /// `log|ΛᵀZᵀWZΛ + I|`.
    pub logdet_u: f64,
    /// `log|R_X|²`, the log determinant of the fixed-effects Schur complement.
    pub logdet_x: f64,
    /// Cholesky factor of the Schur complement onto (other random effects, β).
    pub s_chol: Square,
    /// Per level of the eliminated term: its Cholesky factor, the columns of
    /// the remaining system it touches, and `L⁻¹C` (row-major `q × cols`).
    pub levels: Vec<(Square, Vec<usize>, Vec<f64>)>,
}

/// The random-effects structure of a model.
#[derive(Debug, Clone)]
pub(crate) struct Structure<'a> {
    pub n: usize,
    pub x: &'a Matrix,
    pub terms: Vec<Term<'a>>,
    /// The term eliminated level by level.
    pub big: usize,
    /// Offset of each other term in the remaining system (unused for `big`).
    pub rest_start: Vec<usize>,
    /// Number of remaining random effects.
    pub r_u: usize,
    /// Rows of each level of the eliminated term.
    pub rows_by_level: Vec<Vec<usize>>,
    /// Total number of random effects.
    pub q_total: usize,
    pub n_theta: usize,
}

impl<'a> Structure<'a> {
    pub fn new(n: usize, x: &'a Matrix, terms: Vec<Term<'a>>) -> Self {
        let big = (0..terms.len()).max_by_key(|&k| (terms[k].g * terms[k].q, std::cmp::Reverse(k))).unwrap_or(0);
        let mut rest_start = vec![usize::MAX; terms.len()];
        let mut r_u = 0;
        for (k, t) in terms.iter().enumerate() {
            if k != big {
                rest_start[k] = r_u;
                r_u += t.g * t.q;
            }
        }
        let mut rows_by_level = vec![Vec::new(); terms[big].g];
        for i in 0..n {
            rows_by_level[terms[big].codes[i]].push(i);
        }
        let q_total = terms.iter().map(|t| t.g * t.q).sum();
        let n_theta = terms.iter().map(Term::n_theta).sum();
        Self { n, x, terms, big, rest_start, r_u, rows_by_level, q_total, n_theta }
    }

    /// θ at which every `T` is the identity.
    pub fn theta_start(&self) -> Vec<f64> {
        let mut theta = vec![0.0; self.n_theta];
        for t in &self.terms {
            for i in t.diagonal_indices() {
                theta[i] = 1.0;
            }
        }
        theta
    }

    /// Lower bounds of θ: 0 on the diagonals of `T`.
    pub fn theta_lower(&self) -> Vec<f64> {
        let mut lower = vec![f64::NEG_INFINITY; self.n_theta];
        for t in &self.terms {
            for i in t.diagonal_indices() {
                lower[i] = 0.0;
            }
        }
        lower
    }

    /// `aᵢₖ = Tₖᵀ zᵢₖ` for every row and term (`n × qₖ`, row-major).
    pub fn a_rows(&self, theta: &[f64]) -> Vec<Vec<f64>> {
        self.terms
            .iter()
            .map(|t| {
                let f = t.factor(theta);
                let q = t.q;
                let mut a = vec![0.0; self.n * q];
                for i in 0..self.n {
                    for c in 0..q {
                        let mut s = 0.0;
                        for r in c..q {
                            s += f[r * q + c] * t.z[(i, r)];
                        }
                        a[i * q + c] = s;
                    }
                }
                a
            })
            .collect()
    }

    /// `(ZΛu)ᵢ` for every row.
    pub fn random_part(&self, a: &[Vec<f64>], u: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.n];
        for (k, t) in self.terms.iter().enumerate() {
            for i in 0..self.n {
                let base = t.u_start + t.codes[i] * t.q;
                let ai = &a[k][i * t.q..(i + 1) * t.q];
                out[i] += ai.iter().zip(&u[base..base + t.q]).map(|(x, y)| x * y).sum::<f64>();
            }
        }
        out
    }

    /// `(Xβ)ᵢ` for every row.
    pub fn fixed_part(&self, beta: &[f64]) -> Vec<f64> {
        (0..self.n).map(|i| (0..beta.len()).map(|j| self.x[(i, j)] * beta[j]).sum()).collect()
    }

    /// Entries of row `i` in the remaining system: other terms' effects, then
    /// (with `beta`) the fixed-effects columns.
    fn rest_row(&self, a: &[Vec<f64>], i: usize, beta: bool, out: &mut Vec<(usize, f64)>) {
        out.clear();
        for (k, t) in self.terms.iter().enumerate() {
            if k == self.big {
                continue;
            }
            let base = self.rest_start[k] + t.codes[i] * t.q;
            for c in 0..t.q {
                out.push((base + c, a[k][i * t.q + c]));
            }
        }
        if beta {
            for j in 0..self.x.ncols() {
                out.push((self.r_u + j, self.x[(i, j)]));
            }
        }
    }

    /// Solve the penalized least-squares problem with weights `w` and working
    /// response `v`, for u and (with `beta`) β. `None` if a factorisation
    /// fails (collinear fixed effects).
    pub fn solve(&self, a: &[Vec<f64>], w: &[f64], v: &[f64], beta: bool) -> Option<Solution> {
        let p = if beta { self.x.ncols() } else { 0 };
        let r = self.r_u + p;
        let big = &self.terms[self.big];
        let q1 = big.q;
        let mut s = Square::zeros(r);
        for i in 0..self.r_u {
            *s.at(i, i) = 1.0;
        }
        let mut s_rhs = vec![0.0; r];
        let mut row = Vec::new();
        for i in 0..self.n {
            if w[i] == 0.0 {
                continue;
            }
            self.rest_row(a, i, beta, &mut row);
            for &(ia, va) in &row {
                s_rhs[ia] += w[i] * va * v[i];
                let wa = w[i] * va;
                for &(ib, vb) in &row {
                    if ib <= ia {
                        *s.at(ia, ib) += wa * vb;
                    }
                }
            }
        }
        // Eliminate the big term level by level.
        let mut pos = vec![usize::MAX; r];
        let mut levels = Vec::with_capacity(big.g);
        let mut e_all = Vec::with_capacity(big.g);
        for rows in &self.rows_by_level {
            let mut b = Square::identity(q1);
            let mut rhs = vec![0.0; q1];
            let mut cols: Vec<usize> = Vec::new();
            let mut c_loc: Vec<f64> = Vec::new(); // column-major: col m, effect c at m*q1 + c
            for &i in rows {
                if w[i] == 0.0 {
                    continue;
                }
                let ai = &a[self.big][i * q1..(i + 1) * q1];
                for c in 0..q1 {
                    rhs[c] += w[i] * ai[c] * v[i];
                    for d in 0..=c {
                        *b.at(c, d) += w[i] * ai[c] * ai[d];
                    }
                }
                self.rest_row(a, i, beta, &mut row);
                for &(ia, va) in &row {
                    if pos[ia] == usize::MAX {
                        pos[ia] = cols.len();
                        cols.push(ia);
                        c_loc.extend(std::iter::repeat_n(0.0, q1));
                    }
                    let m = pos[ia];
                    for c in 0..q1 {
                        c_loc[m * q1 + c] += w[i] * ai[c] * va;
                    }
                }
            }
            let l = cholesky(&b)?;
            // D = L⁻¹C, column by column; e = L⁻¹ rhs.
            for m in 0..cols.len() {
                forward(&l, &mut c_loc[m * q1..(m + 1) * q1]);
            }
            forward(&l, &mut rhs);
            for (ma, &ca) in cols.iter().enumerate() {
                let da = &c_loc[ma * q1..(ma + 1) * q1];
                s_rhs[ca] -= da.iter().zip(&rhs).map(|(x, y)| x * y).sum::<f64>();
                for (mb, &cb) in cols.iter().enumerate() {
                    if cb <= ca {
                        let db = &c_loc[mb * q1..(mb + 1) * q1];
                        *s.at(ca, cb) -= da.iter().zip(db).map(|(x, y)| x * y).sum::<f64>();
                    }
                }
            }
            for &c in &cols {
                pos[c] = usize::MAX;
            }
            levels.push((l, cols, c_loc));
            e_all.push(rhs);
        }
        let s_chol = cholesky(&s)?;
        let mut y = s_rhs;
        forward(&s_chol, &mut y);
        backward(&s_chol, &mut y);
        // Back-substitute the big term.
        let mut u = vec![0.0; self.q_total];
        for (j, ((l, cols, d), e)) in levels.iter().zip(&e_all).enumerate() {
            let mut t = e.clone();
            for (m, &c) in cols.iter().enumerate() {
                for k in 0..q1 {
                    t[k] -= d[m * q1 + k] * y[c];
                }
            }
            backward(l, &mut t);
            u[big.u_start + j * q1..big.u_start + (j + 1) * q1].copy_from_slice(&t);
        }
        for (k, term) in self.terms.iter().enumerate() {
            if k != self.big {
                let n = term.g * term.q;
                u[term.u_start..term.u_start + n].copy_from_slice(&y[self.rest_start[k]..self.rest_start[k] + n]);
            }
        }
        let logdet_u = levels.iter().map(|(l, _, _)| log_det(l, 0..q1)).sum::<f64>() + log_det(&s_chol, 0..self.r_u);
        let logdet_x = log_det(&s_chol, self.r_u..r);
        let beta_v = y[self.r_u..].to_vec();
        Some(Solution { u, beta: beta_v, logdet_u, logdet_x, s_chol, levels })
    }

    /// `(XᵀV⁻¹X)⁻¹` up to the scale: the inverse of the fixed-effects Schur
    /// complement of a solution that included β.
    pub fn beta_unscaled_covariance(&self, sol: &Solution) -> Square {
        let p = sol.beta.len();
        let mut block = Square::zeros(p);
        for i in 0..p {
            for j in 0..=i {
                *block.at(i, j) = sol.s_chol.get(self.r_u + i, self.r_u + j);
            }
        }
        inverse_from_cholesky(&block)
    }

    /// Conditional covariance blocks of u given the data, `(ΛᵀZᵀWZΛ + I)⁻¹`,
    /// for each term and level (`q × q`), from a solution without β. Blocks
    /// are NaN when the remaining system is too large to invert.
    pub fn conditional_blocks(&self, sol: &Solution) -> Vec<Vec<Square>> {
        let r = self.r_u;
        let s_inv = if r <= 4000 { Some(inverse_from_cholesky(&sol.s_chol)) } else { None };
        let mut out = Vec::with_capacity(self.terms.len());
        for (k, t) in self.terms.iter().enumerate() {
            let q = t.q;
            let mut blocks = Vec::with_capacity(t.g);
            for j in 0..t.g {
                let mut m = Square::zeros(q);
                if k == self.big {
                    let (l, cols, d) = &sol.levels[j];
                    // L⁻ᵀ (I + D S⁻¹ Dᵀ) L⁻¹
                    let mut inner = Square::identity(q);
                    match &s_inv {
                        Some(si) => {
                            for a in 0..q {
                                for b in 0..q {
                                    let mut sum = 0.0;
                                    for (ma, &ca) in cols.iter().enumerate() {
                                        for (mb, &cb) in cols.iter().enumerate() {
                                            sum += d[ma * q + a] * si.get(ca, cb) * d[mb * q + b];
                                        }
                                    }
                                    *inner.at(a, b) += sum;
                                }
                            }
                        }
                        None if !cols.is_empty() => inner.a.iter_mut().for_each(|v| *v = f64::NAN),
                        None => {}
                    }
                    // Columns of L⁻¹: solve, then form L⁻ᵀ inner L⁻¹.
                    let mut linv = Square::zeros(q);
                    for c in 0..q {
                        let mut e = vec![0.0; q];
                        e[c] = 1.0;
                        forward(l, &mut e);
                        for rr in 0..q {
                            *linv.at(rr, c) = e[rr];
                        }
                    }
                    for a in 0..q {
                        for b in 0..q {
                            let mut sum = 0.0;
                            for x in 0..q {
                                for y in 0..q {
                                    sum += linv.get(x, a) * inner.get(x, y) * linv.get(y, b);
                                }
                            }
                            *m.at(a, b) = sum;
                        }
                    }
                } else {
                    let base = self.rest_start[k] + j * q;
                    for a in 0..q {
                        for b in 0..q {
                            *m.at(a, b) = s_inv.as_ref().map_or(f64::NAN, |si| si.get(base + a, base + b));
                        }
                    }
                }
                blocks.push(m);
            }
            out.push(blocks);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two crossed terms (one with a slope) solved by elimination agree with
    /// the dense normal equations.
    #[test]
    fn elimination_matches_dense_normal_equations() {
        let n = 24;
        let x =
            Matrix::from_columns(&[vec![1.0; n], (0..n).map(|i| (i as f64 * 0.37).sin()).collect::<Vec<_>>()]).unwrap();
        let codes1: Vec<usize> = (0..n).map(|i| i % 5).collect();
        let codes2: Vec<usize> = (0..n).map(|i| i % 3).collect();
        let z1 = Matrix::from_columns(&[vec![1.0; n], (0..n).map(|i| i as f64 / 10.0).collect::<Vec<_>>()]).unwrap();
        let z2 = Matrix::from_columns(&[vec![1.0; n]]).unwrap();
        let terms = vec![
            Term { q: 2, g: 5, codes: &codes1, z: &z1, correlated: true, theta_start: 0, u_start: 0 },
            Term { q: 1, g: 3, codes: &codes2, z: &z2, correlated: true, theta_start: 3, u_start: 10 },
        ];
        let st = Structure::new(n, &x, terms);
        assert_eq!((st.big, st.r_u, st.q_total, st.n_theta), (0, 3, 13, 4));
        let theta = [0.8, -0.3, 0.5, 1.4];
        let a = st.a_rows(&theta);
        let w: Vec<f64> = (0..n).map(|i| 0.5 + (i % 4) as f64 * 0.25).collect();
        let v: Vec<f64> = (0..n).map(|i| (i as f64 * 1.3).cos() * 3.0 + 1.0).collect();
        let sol = st.solve(&a, &w, &v, true).unwrap();
        // Dense: columns [ZΛ (13), X (2)].
        let m = 15;
        let mut zl = vec![vec![0.0; m]; n];
        for i in 0..n {
            for (k, t) in st.terms.iter().enumerate() {
                for c in 0..t.q {
                    zl[i][t.u_start + t.codes[i] * t.q + c] = a[k][i * t.q + c];
                }
            }
            zl[i][13] = x[(i, 0)];
            zl[i][14] = x[(i, 1)];
        }
        let mut big = Square::zeros(m);
        let mut rhs = vec![0.0; m];
        for i in 0..n {
            for p in 0..m {
                rhs[p] += w[i] * zl[i][p] * v[i];
                for q in 0..m {
                    *big.at(p, q) += w[i] * zl[i][p] * zl[i][q];
                }
            }
        }
        for p in 0..13 {
            *big.at(p, p) += 1.0;
        }
        let l = cholesky(&big).unwrap();
        let mut y = rhs.clone();
        forward(&l, &mut y);
        backward(&l, &mut y);
        for p in 0..13 {
            assert!((y[p] - sol.u[p]).abs() < 1e-10, "u[{p}]");
        }
        assert!((y[13] - sol.beta[0]).abs() < 1e-10 && (y[14] - sol.beta[1]).abs() < 1e-10);
        assert!((log_det(&l, 0..15) - sol.logdet_u - sol.logdet_x).abs() < 1e-9);
        let uu = cholesky(&big.sub(&(0..13).collect::<Vec<_>>())).unwrap();
        assert!((log_det(&uu, 0..13) - sol.logdet_u).abs() < 1e-9);
        // Conditional covariance blocks equal blocks of the inverse of the u-part.
        let sol_u = st.solve(&a, &w, &v, false).unwrap();
        let inv = inverse_from_cholesky(&uu);
        let blocks = st.conditional_blocks(&sol_u);
        for (k, t) in st.terms.iter().enumerate() {
            for j in 0..t.g {
                for c in 0..t.q {
                    for d in 0..t.q {
                        let want = inv.get(t.u_start + j * t.q + c, t.u_start + j * t.q + d);
                        assert!((blocks[k][j].get(c, d) - want).abs() < 1e-10, "term {k} level {j}");
                    }
                }
            }
        }
        // β covariance: inverse of the Schur complement onto X.
        let full_inv = inverse_from_cholesky(&l);
        let cov = st.beta_unscaled_covariance(&sol);
        for i in 0..2 {
            for j in 0..2 {
                assert!((cov.get(i, j) - full_inv.get(13 + i, 13 + j)).abs() < 1e-10);
            }
        }
    }
}

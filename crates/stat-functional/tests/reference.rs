//! Basis systems, penalized smoothing, GCV, functional PCA and landmark
//! warps compared with scikit-fda 0.10.1, used as a black box; see
//! `tests/data/reference_cases.py`, which generates synthetic curves with
//! fixed seeds and writes `reference_cases.json`.
//!
//! Conventions (converted in the script, explained there):
//! * scikit-fda's Fourier functions take their phase from `t = 0`, ours from
//!   the left end `a`: on `[0, T]` the bases coincide; elsewhere fitted
//!   values and degrees of freedom (invariant to the rotation within each
//!   harmonic) are compared.
//! * scikit-fda's FPCA eigenvalues (scikit-learn's `explained_variance_`)
//!   have divisor `N − 1`; ours `N`. The script rescales them.
//! * Eigenfunction and score signs are arbitrary: one sign per component is
//!   fixed from the eigenfunction coefficients and applied to both.
//! * GCV is computed in the script from scikit-fda's fits (its own GCV
//!   scorer divides residuals by `1 − Sₘₘ` as well, unlike its documented
//!   formula).

use stat_data::{JsonValue, parse_json};
use stat_functional::{
    BSplineBasis, Basis, CurveData, FourierBasis, FunctionalData, Roughness, SmoothOptions, Smoothing, functional_pca,
    landmark_warps, smooth_curves,
};

fn data() -> JsonValue {
    parse_json(include_str!("data/reference_cases.json")).unwrap()
}

fn key<'a>(v: &'a JsonValue, k: &str) -> &'a JsonValue {
    v.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn field(v: &JsonValue, k: &str) -> f64 {
    num(key(v, k))
}

fn vector(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn matrix(v: &JsonValue) -> Vec<Vec<f64>> {
    v.as_array().unwrap().iter().map(vector).collect()
}

fn vec_of(v: &JsonValue, k: &str) -> Vec<f64> {
    vector(key(v, k))
}

fn mat_of(v: &JsonValue, k: &str) -> Vec<Vec<f64>> {
    matrix(key(v, k))
}

fn range_of(v: &JsonValue) -> (f64, f64) {
    let r = vec_of(v, "range");
    (r[0], r[1])
}

/// `|got − want| ≤ tol`.
fn close_abs(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: got {got}, reference {want} (diff {:e})", (got - want).abs());
}

fn max_abs(m: &[Vec<f64>]) -> f64 {
    m.iter().flatten().fold(0.0_f64, |a, x| a.max(x.abs()))
}

/// Every entry within `rel` times the largest magnitude of `want`, so entries
/// that are zero in exact arithmetic are judged on the scale of the whole
/// matrix rather than on their own rounding.
fn close_matrix(got: &[Vec<f64>], want: &[Vec<f64>], rel: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: rows");
    let tol = rel * max_abs(want).max(1e-300);
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g.len(), w.len(), "{what}: columns of row {i}");
        for (j, (x, y)) in g.iter().zip(w).enumerate() {
            close_abs(*x, *y, tol, &format!("{what}[{i}][{j}]"));
        }
    }
}

fn close_rel(got: f64, want: f64, rel: f64, what: &str) {
    close_abs(got, want, rel * want.abs().max(1e-300), what);
}

fn curves(t: &[f64], y: &[Vec<f64>]) -> Vec<CurveData> {
    y.iter().map(|row| CurveData { t: t.to_vec(), y: row.clone() }).collect()
}

/// (a) B-spline values and first and second derivatives, scikit-fda
/// `BSplineBasis(domain_range, n_basis, order)(t, derivative = d)`.
/// Tolerance 1e-13 relative to the largest value of each derivative order
/// (derivatives of a basis on a short range reach ~10³); observed ≤ 1e-15.
/// (b) Gram matrix and `∫ φⱼ″ φₖ″`: both sides integrate the piecewise
/// polynomials exactly (scikit-fda symbolically, ours by Gauss–Legendre),
/// so 1e-13 relative to the largest entry; observed ≤ 1e-15.
#[test]
fn bspline_values_gram_and_penalty_match_skfda() {
    let all = data();
    let cases = key(&all, "bspline").as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for c in cases {
        let range = range_of(c);
        let n_basis = field(c, "n_basis") as usize;
        let order = field(c, "order") as usize;
        let basis = Basis::BSpline(BSplineBasis::equally_spaced(range, n_basis, order).unwrap());
        let label = format!("B-spline {range:?} n={n_basis} k={order}");
        let t = vec_of(c, "t");
        let values = key(c, "values").as_array().unwrap();
        for (d, want) in values.iter().map(matrix).enumerate() {
            let got: Vec<Vec<f64>> = t.iter().map(|&s| basis.derivatives(s, 2)[d].clone()).collect();
            close_matrix(&got, &want, 1e-13, &format!("{label} derivative {d}"));
        }
        close_matrix(&basis.gram(), &mat_of(c, "gram"), 1e-13, &format!("{label} Gram"));
        close_matrix(
            &basis.penalty_matrix(&[0.0, 0.0, 1.0]),
            &mat_of(c, "penalty2"),
            1e-13,
            &format!("{label} D² penalty"),
        );
    }
}

/// (d) Twelve noisy curves on one grid of 51 points, 15 cubic B-splines,
/// penalty `λ∫(D²x)²` at three fixed λ, against scikit-fda `BasisSmoother`
/// (objective `Σ(y − Φc)² + λcᵀRc`, the same λ). Coefficients and fitted
/// values to 1e-10 relative to their largest magnitude (the λ = 10⁻³ system
/// is the worst conditioned and the two sides solve it differently), df and
/// per-curve GCV `n·SSE/(n − df)²` to 1e-11 relative; observed ≤ 1e-15.
#[test]
fn fixed_lambda_smoothing_matches_skfda() {
    let all = data();
    let s = key(&all, "smooth");
    let basis = Basis::BSpline(
        BSplineBasis::equally_spaced(range_of(s), field(s, "n_basis") as usize, field(s, "order") as usize).unwrap(),
    );
    let data = curves(&vec_of(s, "t"), &mat_of(s, "y"));
    let fixed = key(s, "fixed").as_array().unwrap();
    assert_eq!(fixed.len(), 3);
    for f in fixed {
        let lambda = field(f, "lambda");
        let r = smooth_curves(
            &basis,
            &data,
            &SmoothOptions { operator: Roughness::Derivative(2).operator(), smoothing: Smoothing::Fixed(lambda) },
        )
        .unwrap();
        let label = format!("λ = {lambda}");
        assert_eq!(r.lambda, lambda);
        close_matrix(&r.data.coefs, &mat_of(f, "coefs"), 1e-10, &format!("{label} coefficients"));
        close_matrix(&r.fitted, &mat_of(f, "fitted"), 1e-10, &format!("{label} fitted"));
        for (i, (df, gcv)) in r.df.iter().zip(&r.gcv).enumerate() {
            close_rel(*df, field(f, "df"), 1e-11, &format!("{label} df of curve {i}"));
            close_rel(*gcv, vec_of(f, "gcv")[i], 1e-11, &format!("{label} GCV of curve {i}"));
        }
    }
}

/// (e) λ chosen by GCV, `Σᵢ nᵢ SSEᵢ/(nᵢ − dfᵢ)²` minimized over log₁₀ λ;
/// the reference minimizer is SciPy's bounded `minimize_scalar`
/// (xatol 10⁻⁹) over scikit-fda fits. Our golden-section search stops
/// when the bracket is below 10⁻⁵ decades, so log₁₀ λ is compared to 10⁻⁴
/// (absolute; observed 2·10⁻⁶), df to 10⁻⁴ relative (observed 9·10⁻⁷) and,
/// as the criterion is flat at its minimum, the minimum criterion to 10⁻¹⁰
/// relative (observed 10⁻¹²). The second case adds five
/// curves on a different, irregular grid of 37 points, so the criterion
/// sums over two groups of curves with different times and df.
#[test]
fn gcv_lambda_matches_skfda_minimizer() {
    let all = data();
    let s = key(&all, "smooth");
    let g = key(&all, "gcv");
    let basis = Basis::BSpline(
        BSplineBasis::equally_spaced(range_of(s), field(s, "n_basis") as usize, field(s, "order") as usize).unwrap(),
    );
    let first = curves(&vec_of(s, "t"), &mat_of(s, "y"));
    let second_group = key(g, "second_group");
    let second = curves(&vec_of(second_group, "t"), &mat_of(second_group, "y"));
    let cases = key(g, "cases").as_array().unwrap();
    assert_eq!(cases.len(), 2);
    for c in cases {
        let groups = vec_of(c, "groups");
        let mut data = first.clone();
        if groups.len() == 2 {
            data.extend(second.iter().cloned());
        }
        let r = smooth_curves(
            &basis,
            &data,
            &SmoothOptions { operator: Roughness::Derivative(2).operator(), smoothing: Smoothing::Gcv },
        )
        .unwrap();
        let name = key(c, "name").as_str().unwrap();
        close_abs(r.lambda.log10(), field(c, "log10_lambda"), 1e-4, &format!("{name}: log10 λ"));
        let dfs = vec_of(c, "df");
        close_rel(r.df[0], dfs[0], 1e-4, &format!("{name}: df of the first grid"));
        if groups.len() == 2 {
            close_rel(r.df[first.len()], dfs[1], 1e-4, &format!("{name}: df of the second grid"));
        }
        close_rel(r.total_gcv(), field(c, "criterion"), 1e-10, &format!("{name}: minimum GCV"));
    }
}

/// (c) Fourier bases of 11 functions, period 10, with the D² and harmonic
/// acceleration penalties. On `[0, 10]` the bases coincide: basis values to
/// 1e-13 and penalty matrices to 1e-12 relative to their largest entry,
/// coefficients and fitted values to 1e-10. On `[3, 13]` only the
/// rotation-invariant fitted values (1e-10) and df (1e-11) are compared.
/// Observed differences are all ≤ 2e-15.
#[test]
fn fourier_smoothing_matches_skfda() {
    let all = data();
    let f = key(&all, "fourier");
    let y = mat_of(f, "y");
    let cases = key(f, "cases").as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for c in cases {
        let (start, period) = (field(c, "start"), field(c, "period"));
        let n_basis = field(c, "n_basis") as usize;
        let basis = Basis::Fourier(FourierBasis::new((start, start + period), n_basis, period).unwrap());
        let operator = match key(c, "operator").as_str().unwrap() {
            "d2" => Roughness::Derivative(2).operator(),
            _ => Roughness::HarmonicAcceleration { period }.operator(),
        };
        let lambda = field(c, "lambda");
        let label = format!("Fourier from {start}, {:?}, λ = {lambda}", key(c, "operator").as_str().unwrap());
        let t = vec_of(c, "t");
        let r = smooth_curves(
            &basis,
            &curves(&t, &y),
            &SmoothOptions { operator: operator.clone(), smoothing: Smoothing::Fixed(lambda) },
        )
        .unwrap();
        close_matrix(&r.fitted, &mat_of(c, "fitted"), 1e-10, &format!("{label}: fitted"));
        for df in &r.df {
            close_rel(*df, field(c, "df"), 1e-11, &format!("{label}: df"));
        }
        if let Some(values) = c.get("values") {
            for (d, want) in values.as_array().unwrap().iter().map(matrix).enumerate() {
                let got: Vec<Vec<f64>> = t[..want.len()].iter().map(|&s| basis.derivatives(s, 2)[d].clone()).collect();
                close_matrix(&got, &want, 1e-13, &format!("{label}: derivative {d}"));
            }
            close_matrix(&basis.penalty_matrix(&operator), &mat_of(c, "penalty"), 1e-12, &format!("{label}: penalty"));
            close_matrix(&r.data.coefs, &mat_of(c, "coefs"), 1e-10, &format!("{label}: coefficients"));
        }
    }
}

/// (f) Functional PCA of the λ = 0.1 smooths (12 curves, 15 cubic
/// B-splines), scikit-fda `FPCA(n_components = 3)` on the `FDataBasis`:
/// both are the exact basis computation (eigen-decomposition of `LᵀVL`,
/// `W = LLᵀ`), so eigenvalues (scikit-fda's rescaled to divisor N),
/// proportions of variance (ours over all components, scikit-learn's over
/// all of `CL`'s variance: the same total), eigenfunctions on 21 points,
/// eigenfunction coefficients and scores agree to 1e-11 relative to each
/// quantity's largest magnitude (observed ≤ 1e-14), up to one sign per
/// component.
#[test]
fn fpca_matches_skfda() {
    let all = data();
    let s = key(&all, "smooth");
    let p = key(&all, "fpca");
    let basis = Basis::BSpline(
        BSplineBasis::equally_spaced(range_of(s), field(s, "n_basis") as usize, field(s, "order") as usize).unwrap(),
    );
    let fd = FunctionalData { basis, coefs: mat_of(p, "coefs") };
    let fit = functional_pca(&fd, 3).unwrap();
    let want_values = vec_of(p, "eigenvalues");
    for (k, w) in want_values.iter().enumerate() {
        close_rel(fit.eigenvalues[k], *w, 1e-11, &format!("eigenvalue {k}"));
        close_rel(fit.proportion(k), vec_of(p, "explained_variance_ratio")[k], 1e-11, &format!("proportion {k}"));
    }
    for (j, (m, w)) in fit.mean.iter().zip(vec_of(p, "mean")).enumerate() {
        close_abs(*m, w, 1e-12 * max_abs(&fd.coefs), &format!("mean coefficient {j}"));
    }
    let want_harmonics = mat_of(p, "harmonics");
    let signs: Vec<f64> = (0..3)
        .map(|k| {
            let d: f64 = fit.harmonics[k].iter().zip(&want_harmonics[k]).map(|(a, b)| a * b).sum();
            d.signum()
        })
        .collect();
    let flip = |m: &[Vec<f64>]| -> Vec<Vec<f64>> {
        m.iter().enumerate().map(|(k, r)| r.iter().map(|x| x * signs[k]).collect()).collect()
    };
    close_matrix(&flip(&fit.harmonics), &want_harmonics, 1e-11, "eigenfunction coefficients");
    let grid = vec_of(p, "grid");
    let got_eigenfunctions: Vec<Vec<f64>> = fit.harmonics.iter().map(|h| fd.eval_coefs(h, &grid, 0)).collect();
    close_matrix(&flip(&got_eigenfunctions), &mat_of(p, "eigenfunctions"), 1e-11, "eigenfunctions on the grid");
    let got_scores: Vec<Vec<f64>> =
        fit.scores.iter().map(|r| r.iter().zip(&signs).map(|(x, s)| x * s).collect()).collect();
    close_matrix(&got_scores, &mat_of(p, "scores"), 1e-11, "scores");
}

/// (g) Landmark warps for five curves with three landmarks each, aligned
/// to the mean landmarks and to given locations, against scikit-fda
/// `landmark_elastic_registration_warping` (monotone cubic PCHIP through
/// `(a, a)`, `(t₀ₗ, tᵢₗ)`, `(b, b)`) evaluated at our 401 warp grid points,
/// where our stored values are the interpolant itself: 1e-12 absolute on a
/// range of length 10 (observed ≤ 1e-14).
#[test]
fn landmark_warps_match_skfda() {
    let all = data();
    let l = key(&all, "landmark");
    let range = range_of(l);
    let grid = vec_of(l, "grid");
    let landmarks = mat_of(l, "landmarks");
    let location = vec_of(l, "location");
    for (loc, k) in [(None, "warps"), (Some(location.as_slice()), "warps_location")] {
        let (warps, used) = landmark_warps(range, &landmarks, loc).unwrap();
        if let Some(loc) = loc {
            assert_eq!(used, loc);
        }
        let want = mat_of(l, k);
        assert_eq!(warps.len(), want.len());
        for (i, (w, r)) in warps.iter().zip(&want).enumerate() {
            assert_eq!(w.grid.len(), grid.len());
            for (g, (&t, &h)) in grid.iter().zip(r).enumerate() {
                close_abs(w.grid[g], t, 1e-12, &format!("{k}: grid point {g}"));
                close_abs(w.values[g], h, 1e-12, &format!("{k}: curve {i} at t = {t}"));
                close_abs(w.eval(t), h, 1e-12, &format!("{k}: curve {i} eval at t = {t}"));
            }
        }
    }
}

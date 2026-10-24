# Reference values for the pairwise comparisons added to
# stat_inference::multiple (R 4.3.3, base stats only).
# Run: Rscript crates/stat-inference/tests/data/pairwise_reference.R
g <- list(
  a = c(24.5, 23.5, 26.4, 27.1, 29.9, 25.0),
  b = c(28.4, 34.2, 29.5, 32.2, 30.1),
  c = c(26.1, 28.3, 24.3, 26.2, 27.8),
  d = c(30.0, 27.5, 31.2, 29.9, 28.8, 30.5, 32.1)
)
y <- unlist(g)
f <- factor(rep(names(g), lengths(g)))
k <- length(g); m <- k * (k - 1) / 2
n <- lengths(g); means <- sapply(g, mean); vars <- sapply(g, var)
dfe <- sum(n) - k
mse <- sum((n - 1) * vars) / dfe
pairs <- combn(k, 2)
fmt <- function(x) format(x, digits = 17)

# Pooled t: unadjusted (Fisher LSD), Bonferroni (as pairwise.t.test), Šidák,
# Holm–Šidák.
raw <- apply(pairs, 2, function(p) {
  i <- p[1]; j <- p[2]
  t <- (means[i] - means[j]) / sqrt(mse * (1 / n[i] + 1 / n[j]))
  2 * pt(-abs(t), dfe)
})
pt_bonf <- pairwise.t.test(y, f, p.adjust.method = "bonferroni")$p.value
cat("lsd:", fmt(raw), "\n")
cat("bonferroni (pairwise.t.test lower triangle):", fmt(pt_bonf[lower.tri(pt_bonf, diag = TRUE)]), "\n")
sidak <- 1 - (1 - raw)^m
cat("sidak:", fmt(sidak), "\n")
o <- order(raw)
hs <- numeric(m); run <- 0
for (r in seq_along(o)) { run <- max(run, 1 - (1 - raw[o[r]])^(m - r + 1)); hs[o[r]] <- run }
cat("holm-sidak:", fmt(hs), "\n")
se12 <- sqrt(mse * (1 / n[1] + 1 / n[2]))
cat("bonferroni CI pair 1:", fmt((means[1] - means[2]) + c(-1, 1) * qt(1 - 0.05 / (2 * m), dfe) * se12), "\n")
cat("sidak CI pair 1:", fmt((means[1] - means[2]) + c(-1, 1) * qt(1 - (1 - 0.95^(1 / m)) / 2, dfe) * se12), "\n")

# Newman–Keuls: q against the studentized range with r = span of the pair
# in the ordered means; each p is the largest raw p of the pairs spanning it.
rk <- rank(means)
nk <- apply(pairs, 2, function(p) {
  i <- p[1]; j <- p[2]
  q <- abs(means[i] - means[j]) / sqrt(mse / 2 * (1 / n[i] + 1 / n[j]))
  r <- abs(rk[i] - rk[j]) + 1
  c(ptukey(q, r, dfe, lower.tail = FALSE), min(rk[i], rk[j]), max(rk[i], rk[j]))
})
nk_adj <- sapply(seq_len(m), function(c) max(nk[1, nk[2, ] <= nk[2, c] & nk[3, ] >= nk[3, c]]))
cat("newman-keuls raw:", fmt(nk[1, ]), "\n")
cat("newman-keuls:", fmt(nk_adj), "\n")

# Tamhane T2 and Dunnett T3 (Welch t and df per pair).
smm_sf <- function(c, m, df) {
  integrate(function(s) (1 - (2 * pnorm(c * s) - 1)^m) * dchisq(df * s^2, df) * 2 * df * s,
            0, Inf, rel.tol = 1e-12)$value
}
smm_q <- function(level, m, df) uniroot(function(c) smm_sf(c, m, df) - (1 - level), c(0.5, 20), tol = 1e-12)$root
w <- apply(pairs, 2, function(p) {
  i <- p[1]; j <- p[2]
  tt <- t.test(g[[i]], g[[j]])
  t <- unname(tt$statistic); df <- unname(tt$parameter)
  se <- sqrt(vars[i] / n[i] + vars[j] / n[j])
  c(t, df, 1 - (1 - tt$p.value)^m, smm_sf(abs(t), m, df),
    (means[i] - means[j]) + qt(1 - (1 - 0.95^(1 / m)) / 2, df) * se,
    (means[i] - means[j]) + smm_q(0.95, m, df) * se)
})
cat("welch t:", fmt(w[1, ]), "\n")
cat("welch df:", fmt(w[2, ]), "\n")
cat("tamhane-t2:", fmt(w[3, ]), "\n")
cat("dunnett-t3:", fmt(w[4, ]), "\n")
cat("t2 upper:", fmt(w[5, ]), "\n")
cat("t3 upper:", fmt(w[6, ]), "\n")
# A quantile of the studentized maximum modulus (it lies below the Šidák
# t quantile qt(1 - (1 - 0.95^(1/3))/2, 10) = 2.870, as Šidák's inequality says).
cat("smm(0.95, 3, 10):", fmt(smm_q(0.95, 3, 10)), "\n")

# Linear trend with coefficients -1.5, -0.5, 0.5, 1.5.
cc <- seq_len(k) - (k + 1) / 2
L <- sum(cc * means); se <- sqrt(mse * sum(cc^2 / n)); t <- L / se
ssb <- sum(n * (means - mean(y))^2); ssl <- L^2 / sum(cc^2 / n)
fnl <- (ssb - ssl) / (k - 2) / mse
cat("trend:", fmt(c(L, se, t, 2 * pt(-abs(t), dfe), ssl / ssb, fnl, pf(fnl, k - 2, dfe, lower.tail = FALSE))), "\n")
# The same contrast from a linear model with these coefficients as a
# covariate plus the remaining between-group variation.
fit <- lm(y ~ f)
cat("lm check (contrast t equals):", fmt(sum(cc * coef(lm(y ~ f - 1))) / sqrt(sum(cc^2 * diag(vcov(lm(y ~ f - 1)))))), "\n")

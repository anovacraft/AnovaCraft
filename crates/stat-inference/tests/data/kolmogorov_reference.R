# Reference values for stat_inference::kolmogorov (R 4.3.3).
# Run: Rscript crates/stat-inference/tests/data/kolmogorov_reference.R
a <- c(0.61, 0.29, 0.06, 0.59, -1.73, -0.74, 0.51, -0.56)
b <- c(2.2, 1.22, 0.32, 2.98, 0.69, 1.55, -0.08, 1.06, 0.86, 1.82)
for (alt in c("two.sided", "less", "greater")) {
  t <- ks.test(a, b, alternative = alt)
  cat(alt, "exact:", format(c(t$statistic, t$p.value), digits = 17), "\n")
  t <- ks.test(a, b, alternative = alt, exact = FALSE)
  cat(alt, "asymptotic:", format(c(t$statistic, t$p.value), digits = 17), "\n")
}
x <- c(1, 2, 2, 3, 4, 5)
y <- c(2, 3, 5, 6, 7, 7, 8)
t <- suppressWarnings(ks.test(x, y, exact = FALSE))
cat("ties asymptotic:", format(c(t$statistic, t$p.value), digits = 17), "\n")

# Reference values for stat_inference::pvalues (R 4.x, base stats only).
# Run: Rscript crates/stat-inference/tests/data/pvalues_reference.R
p <- c(0.01, 0.04, 0.03, 0.005, 0.5, 0.2, 0.04, 0.8, 0.001, 0.07)
for (m in c("bonferroni", "holm", "hochberg", "BH", "BY")) {
  cat(m, ":", format(p.adjust(p, m), digits = 17), "\n")
}
cat("sidak:", format(1 - (1 - p)^length(p), digits = 17), "\n")
# Storey (2002) q-values with lambda = 0.5, by hand.
lambda <- 0.5
pi0 <- min(1, mean(p > lambda) / (1 - lambda))
cat("pi0:", pi0, "\n")
cat("q:", format(pmin(1, pi0 * p.adjust(p, "BH")), digits = 17), "\n")
# Combining p-values.
q <- c(0.01, 0.2, 0.3, 0.04)
x2 <- -2 * sum(log(q))
cat("fisher:", format(c(x2, pchisq(x2, 2 * length(q), lower.tail = FALSE)), digits = 17), "\n")
z <- sum(qnorm(1 - q)) / sqrt(length(q))
cat("stouffer:", format(c(z, pnorm(z, lower.tail = FALSE)), digits = 17), "\n")
w <- c(10, 20, 30, 40)
zw <- sum(w * qnorm(1 - q)) / sqrt(sum(w^2))
cat("weighted:", format(c(zw, pnorm(zw, lower.tail = FALSE)), digits = 17), "\n")

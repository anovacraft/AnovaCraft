# Reference values for crates/stat-bayes/tests/sampler_reference.rs (PSIS-LOO
# and WAIC). R 4.3 with loo 2.6.0 (Debian r-cran-loo).
suppressMessages(library(loo))
options(digits = 15)
S <- 400
s <- 1:S
mu <- 0.2 + 0.4 * sin(s * 1.7)
sigma <- 1 + 0.3 * cos(s * 0.37)^2
y <- c(-0.8, 0.1, 0.5, 1.2, -0.3, 0.9, 0.0, 0.4, -1.1, 0.7, 2.0, -0.2, 0.3, 6.5, 0.6)
ll <- sapply(y, function(v) dnorm(v, mu, sigma, log = TRUE))
l <- loo(ll, r_eff = rep(1, length(y)))
print(l$estimates)
cat("pareto_k", l$diagnostics$pareto_k, "\n")
cat("pointwise", l$pointwise[, "elpd_loo"], "\n")
w <- suppressWarnings(waic(ll))
print(w$estimates)

# Reference values for stat_inference::effect_sizes (R 4.x, base stats only).
# Noncentrality bounds by uniroot on pf / pchisq in the noncentrality
# (Steiger 2004). Run: Rscript crates/stat-inference/tests/data/effect_sizes_reference.R
f_bounds <- function(f, df1, df2, level = 0.95) {
  a <- 1 - level
  g <- function(target) {
    if (pf(f, df1, df2, 0) <= target) return(0)
    uniroot(function(l) pf(f, df1, df2, l) - target, c(0, 1000), tol = 1e-12)$root
  }
  c(g(1 - a / 2), g(a / 2))
}
chi_bounds <- function(x, df, level = 0.95) {
  a <- 1 - level
  g <- function(target) {
    if (pchisq(x, df, 0) <= target) return(0)
    uniroot(function(l) pchisq(x, df, l) - target, c(0, 1000), tol = 1e-12)$root
  }
  c(g(1 - a / 2), g(a / 2))
}
print(f_bounds(5, 3, 36), digits = 14)
print(f_bounds(0.5, 2, 20), digits = 14)
print(chi_bounds(12, 2), digits = 14)
# One-way example used in app-model (PlantGrowth): aov sums of squares.
fit <- aov(weight ~ group, data = PlantGrowth)
ss <- summary(fit)[[1]][["Sum Sq"]]
fs <- summary(fit)[[1]][["F value"]][1]
print(c(ss, fs), digits = 14)
lam <- f_bounds(fs, 2, 27)
print(c(eta2 = ss[1] / sum(ss), lo = lam[1] / (lam[1] + 30), hi = lam[2] / (lam[2] + 30)), digits = 14)

# Glass's delta on R's sleep data (group 1 against group 2, scaled by the
# second group's standard deviation).
a <- sleep$extra[sleep$group == 1]; b <- sleep$extra[sleep$group == 2]
cat("glass delta:", format((mean(a) - mean(b)) / sd(b), digits = 17), "\n")

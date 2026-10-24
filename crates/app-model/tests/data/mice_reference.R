# Reference values for the pooled analyses in app-model's multiple
# imputation tests (crates/app-model/src/pooling/tests.rs).
#
# Run from this folder with R 4.3 and mice 3.16:
#   Rscript mice_reference.R
# It writes mice_nhanes2_stacked.csv (mice's imputations of the nhanes2
# data, stacked with an Imputation column, 0 = original rows) and prints
# the pooled results the Rust tests compare against. The app never runs R;
# this script only produces the reference values.
suppressMessages(library(mice))
set.seed(1)
imp <- mice(nhanes2, m = 5, seed = 123, printFlag = FALSE)
long <- complete(imp, "long", include = TRUE)
out <- data.frame(
  Imputation = long$.imp, Row = long$.id,
  age = as.character(long$age), bmi = long$bmi,
  hyp = ifelse(is.na(long$hyp), NA, ifelse(long$hyp == "yes", 1, 0)),
  chl = long$chl
)
write.csv(out, "mice_nhanes2_stacked.csv", row.names = FALSE, na = "")
f <- function(x) sprintf("%.17g", x)

cat("# Linear model chl ~ age + bmi\n")
fit1 <- with(imp, lm(chl ~ age + bmi))
fit0 <- with(imp, lm(chl ~ bmi))
fitn <- with(imp, lm(chl ~ 1))
s <- summary(pool(fit1))
p <- pool(fit1)$pooled
b <- p[p$term == "bmi", ]
cat("bmi estimate", f(b$estimate), "ubar", f(b$ubar), "b", f(b$b), "t", f(b$t), "df", f(b$df), "riv", f(b$riv), "fmi", f(b$fmi), "\n")
cat("bmi se", f(s$std.error[s$term == "bmi"]), "p", f(s$p.value[s$term == "bmi"]), "\n")
d1 <- D1(fit1, fit0)$result
cat("age D1 F", f(d1[1]), "df1", f(d1[2]), "df2", f(d1[3]), "p", f(d1[4]), "riv", f(d1[5]), "\n")
d3 <- D3(fit1, fit0)$result
cat("age D3 F", f(d3[1]), "df1", f(d3[2]), "df2", f(d3[3]), "p", f(d3[4]), "riv", f(d3[5]), "\n")
d1 <- D1(fit1, fitn)$result
cat("model D1 F", f(d1[1]), "df1", f(d1[2]), "df2", f(d1[3]), "p", f(d1[4]), "riv", f(d1[5]), "\n")
d3 <- D3(fit1, fitn)$result
cat("model D3 F", f(d3[1]), "df1", f(d3[2]), "df2", f(d3[3]), "p", f(d3[4]), "riv", f(d3[5]), "\n")
r2 <- pool.r.squared(fit1)
cat("r2", f(r2[1, ]), "\n")
r2 <- pool.r.squared(fit1, adjusted = TRUE)
cat("adj r2", f(r2[1, ]), "\n")
# mice rounds the normal quantile to 1.96; the app uses the exact one.
z <- pool.scalar(atanh(sqrt(sapply(getfit(fit1), function(x) summary(x)$r.squared))), rep(1 / (25 - 3), 5))
cat("r2 interval (exact quantile)", f(tanh(z$qbar + c(-1, 1) * qnorm(0.975) * sqrt(z$t))^2), "\n")

cat("# Mean of chl\n")
p <- pool(fitn)$pooled
cat("mean estimate", f(p$estimate), "t", f(p$t), "df", f(p$df), "fmi", f(p$fmi), "\n")

cat("# Logistic regression hyp ~ bmi + chl\n")
# Fitted to convergence: R's default stopping rule leaves the variances
# about 1e-5 from their converged values.
tight <- glm.control(epsilon = 1e-14, maxit = 100)
g <- with(imp, glm(hyp ~ bmi + chl, family = binomial, control = tight))
p <- pool(g)$pooled
for (term in c("(Intercept)", "bmi", "chl")) {
  r <- p[p$term == term, ]
  cat(term, "estimate", f(r$estimate), "t", f(r$t), "df", f(r$df), "fmi", f(r$fmi), "\n")
}
g0 <- with(imp, glm(hyp ~ bmi, family = binomial, control = tight))
d1 <- D1(g, g0)$result
cat("chl D1 F", f(d1[1]), "df1", f(d1[2]), "df2", f(d1[3]), "p", f(d1[4]), "\n")

# ---------------------------------------------------------------------------
# Unit values for crates/stat-inference/src/pooling/tests.rs: Rubin's rules
# and Barnard-Rubin df (pool.scalar, barnard.rubin) and the D1 statistic
# (mitml's .D1, which mice::D1 calls).
cat("# Rubin's rules on constructed estimates\n")
q <- c(1.21, 1.35, 0.98, 1.10, 1.42); u <- c(0.040, 0.052, 0.047, 0.038, 0.045)
for (dfc in c(Inf, 20)) {
  p <- mice::pool.scalar(q, u, n = if (is.infinite(dfc)) Inf else dfc + 1, k = 1)
  cat(sprintf("dfcom=%s qbar=%.17g ubar=%.17g b=%.17g t=%.17g df=%.17g r=%.17g fmi=%.17g\n",
              dfc, p$qbar, p$ubar, p$b, p$t, p$df, p$r, p$fmi))
  br <- mice:::barnard.rubin(5, p$b, p$t, dfc)
  cat(sprintf("  br=%.17g  p=%.17g  lo=%.17g\n", br, 2 * pt(-abs(p$qbar / sqrt(p$t)), br),
              p$qbar - qt(0.975, br) * sqrt(p$t)))
}
Q <- rbind(c(1.0, 2.0, 3.0, 1.5, 2.5), c(0.5, 0.2, 0.9, 0.4, 0.7))
U <- array(0, c(2, 2, 5))
for (l in 1:5) U[, , l] <- matrix(c(0.30 + 0.01 * l, 0.05, 0.05, 0.10 + 0.02 * l), 2)
for (dc in list(NULL, 30)) {
  D <- mitml:::.D1(Q, U, dc)
  cat(sprintf("D1 dfcom=%s F=%.17g v=%.17g r=%.17g p=%.17g\n", if (is.null(dc)) "Inf" else dc,
              D$F, D$v, D$r, pf(D$F, D$k, D$v, lower.tail = FALSE)))
}

# ---------------------------------------------------------------------------
# Unit values for crates/stat-multivariate/src/multiple_imputation/tests.rs:
# mice's Bayesian linear regression draw (.norm.draw) with the chi-square
# draw g and the normal deviates z fixed.
cat("# Bayesian regression draw\n")
x <- cbind(1, c(1.2, 2.5, 3.1, 4.8, 5.0, 6.7, 7.1, 8.4), c(0, 1, 0, 1, 1, 0, 1, 0))
y <- c(2.3, 4.1, 4.9, 8.2, 7.7, 9.9, 11.8, 12.1)
p <- mice:::estimice(x, y)
g <- 3.7; z <- c(0.4, -1.1, 0.25)
sigma.star <- sqrt(sum((p$r)^2) / g)
beta.star <- p$c + (t(chol(mice:::sym(p$v))) %*% z) * sigma.star
cat(sprintf("%.17g ", c(p$c)), "\n")
cat(sprintf("%.17g ", c(beta.star)), "\n")
cat(sprintf("%.17g\n", sigma.star))

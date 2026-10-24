# Reference values for the log-rank test for trend and the number at risk
# (crates/stat-reliability/src/kaplan_meier.rs). R 4.x, survival 3.x.
# The trend statistic is the standard score form (Collett 2015, §2.6.4)
# evaluated from survdiff's observed, expected and variance matrix.
suppressMessages(library(survival))
options(digits = 15)
set.seed(23)
dose <- rep(1:3, c(12, 13, 11))
time <- round(rexp(36, rate = 0.05 * c(1, 1.6, 2.6)[dose]), 0) + 1
status <- as.integer(runif(36) > 0.25)
cat("time <-", deparse(time), "\nstatus <-", deparse(status), "\ndose <-", deparse(dose), "\n")
sd <- survdiff(Surv(time, status) ~ dose)
print(sd$obs); print(sd$exp); print(sd$var); print(sd$chisq)
for (w in list(c(1, 2, 3), c(0, 1, 4))) {
  u <- sum(w * (sd$obs - sd$exp)); v <- drop(t(w) %*% sd$var %*% w)
  cat("scores", w, "U", u, "Var", v, "chisq", u^2 / v, "p", pchisq(u^2 / v, 1, lower.tail = FALSE), "\n")
}
# Number at risk and cumulative censored counts at given times.
fit <- survfit(Surv(time, status) ~ dose)
s <- summary(fit, times = c(0, 10, 20, 30, 40), extend = TRUE)
print(data.frame(strata = s$strata, time = s$time, n.risk = s$n.risk, n.censor = s$n.censor))

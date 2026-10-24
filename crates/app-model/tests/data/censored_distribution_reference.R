# Reference values for the censored Distribution report (app_model::censored_distribution).
# Run: Rscript censored_distribution_reference.R   (R packages survival, fitdistrplus)
# Data: the "Well water arsenic" sample (invented): value = measured concentration or,
# for a non-detect, its detection limit; nd = 1 marks a non-detect (left-censored).
library(survival); library(fitdistrplus)
value <- c(1, 1, 3.2, 2, 4.8, 1, 6.1, 2.4, 2, 9.7, 1.6, 5, 12.5, 3.9, 2, 7.3, 1.2, 5, 2.9, 18.4, 1, 4.1, 8.8, 2.2)
nd    <- c(1, 1, 0,   1, 0,   1, 0,   0,   1, 0,   0,   1, 0,    0,   0, 0,   0,   1, 0,   0,    1, 0,   0,   0)
left  <- ifelse(nd == 1, NA, value)
cd <- data.frame(left = left, right = value)
options(digits = 15)
for (d in c("norm", "lnorm", "weibull", "gamma", "exp", "logis")) {
  f <- fitdistcens(cd, d, control = list(reltol = 1e-15, maxit = 10000))
  cat(d, "estimate:", format(f$estimate, digits = 15), "loglik:", format(f$loglik, digits = 15), "\n")
}
# Reversed Kaplan-Meier: flip about M = 25, fit Kaplan-Meier, flip back.
M <- 25
km <- survfit(Surv(M - value, 1 - nd) ~ 1)
cdf <- data.frame(x = M - km$time, F = km$surv)  # F(x) = P(X < x) at detected values
print(cdf[order(cdf$x), ])
# Turnbull NPMLE through interval2 (left: NA lower end). survfit closes the non-detect's
# interval at its limit, so a value detected at 2 shares the "< 2" interval and the
# estimate differs from the reversed KM; the report keeps "< d" open at d, giving the KM.
tb <- survfit(Surv(left, value, type = "interval2") ~ 1)
print(data.frame(time = tb$time, surv = tb$surv))
# Mixed censoring (right-censored above 15 for the largest value, interval 3-4 for row 3)
left2 <- left; right2 <- value
left2[20] <- 15; right2[20] <- NA
left2[3] <- 3; right2[3] <- 4
f2 <- fitdistcens(data.frame(left = left2, right = right2), "lnorm", control = list(reltol = 1e-15, maxit = 10000))
cat("mixed lnorm:", format(f2$estimate, digits = 15), "loglik:", format(f2$loglik, digits = 15), "\n")

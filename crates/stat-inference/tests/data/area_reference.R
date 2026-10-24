# Reference values for crates/stat-inference/src/area.rs: trapezoid area of
# mean responses with Bailer's (1988) standard error, written out from the
# definitions (R 4.x, base only).
options(digits = 16)
x <- c(0, 0, 0, 1, 1, 1, 2, 2, 2, 4, 4, 4, 8, 8, 8)
y <- c(2, 3, 2.5, 20, 24, 19, 15, 13, 17, 9, 7, 8.5, 3, 2, 4)
xs <- sort(unique(x)); m <- length(xs)
ybar <- tapply(y, x, mean); s2 <- tapply(y, x, var); n <- tapply(y, x, length)
c_w <- (c(diff(xs), 0) + c(0, diff(xs))) / 2
span <- xs[m] - xs[1]
auc <- sum(c_w * ybar); se <- sqrt(sum(c_w^2 * s2 / n))
cat("AUC", auc, "SE", se, "\n")
# Cross-check of the trapezoid sum against an independent integration.
cat("approx check", integrate(approxfun(xs, ybar), 0, 8, subdivisions = 1000)$value, "\n")
# Baseline = mean of the first two points: weights shift by span / 2.
w <- c_w; w[1:2] <- w[1:2] - span / 2
cat("AUC_B", sum(w * ybar), "SE_B", sqrt(sum(w^2 * s2 / n)), "baseline", mean(ybar[1:2]), "\n")

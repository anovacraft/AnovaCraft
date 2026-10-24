# Reference values for crates/stat-inference/src/repeated.rs (sphericity,
# Greenhouse–Geisser and Huynh–Feldt corrections, factorial repeated
# measures). Run with `Rscript repeated_reference.R` (R 4.x, car 3.x).
suppressMessages(library(car))
options(digits = 15, contrasts = c("contr.sum", "contr.poly"))

cat("== One-way: six subjects, three conditions ==\n")
y <- rbind(c(8.0, 7.2, 6.1), c(9.1, 8.5, 7.0), c(7.5, 7.9, 6.8),
           c(8.8, 8.0, 7.9), c(9.5, 8.1, 7.2), c(7.9, 7.0, 7.1))
m1 <- lm(y ~ 1)
idata <- data.frame(cond = factor(1:3))
s1 <- summary(Anova(m1, idata = idata, idesign = ~cond, type = 3), multivariate = FALSE)
print(s1$univariate.tests); print(s1$sphericity.tests); print(s1$pval.adjustments)
print(mauchly.test(m1, X = ~1))

cat("== One-way: five conditions (Mauchly's second-order term) ==\n")
y5 <- rbind(c(31, 29, 25, 27, 24), c(28, 27, 26, 22, 20), c(35, 33, 30, 31, 25),
            c(30, 31, 26, 25, 25), c(26, 25, 25, 20, 19), c(33, 29, 30, 27, 21),
            c(29, 26, 27, 24, 23), c(32, 33, 27, 28, 22), c(27, 24, 23, 22, 21),
            c(34, 30, 31, 26, 24))
m5 <- lm(y5 ~ 1)
s5 <- summary(Anova(m5, idata = data.frame(cond = factor(1:5)), idesign = ~cond, type = 3), multivariate = FALSE)
print(s5$univariate.tests); print(s5$sphericity.tests); print(s5$pval.adjustments)
# car and stats::mauchly.test write 3p (cells) for 3d (contrast dimensions)
# in the second-order term; Anderson's formula with d:
W <- s5$sphericity.tests[1, 1]; n <- 9; d <- 4
rho <- 1 - (2 * d^2 + d + 2) / (6 * d * n)
w2 <- (d + 2) * (d - 1) * (d - 2) * (2 * d^3 + 6 * d^2 + 3 * d + 2) / (288 * (n * d * rho)^2)
z <- -n * rho * log(W); f <- d * (d + 1) / 2 - 1
cat("Anderson chi-square", z, "p", pchisq(z, f, lower.tail = FALSE) +
    w2 * (pchisq(z, f + 4, lower.tail = FALSE) - pchisq(z, f, lower.tail = FALSE)), "\n")

cat("== O'Brien & Kaiser (1985): treatment x gender between, phase x hour within ==\n")
data(OBrienKaiser)
phase <- factor(rep(c("pretest", "posttest", "followup"), c(5, 5, 5)),
                levels = c("pretest", "posttest", "followup"))
hour <- ordered(rep(1:5, 3))
mod <- lm(cbind(pre.1, pre.2, pre.3, pre.4, pre.5, post.1, post.2, post.3, post.4, post.5,
                fup.1, fup.2, fup.3, fup.4, fup.5) ~ treatment * gender, data = OBrienKaiser)
s <- summary(Anova(mod, idata = data.frame(phase, hour), idesign = ~phase * hour, type = 3),
             multivariate = FALSE)
print(s$univariate.tests); print(s$sphericity.tests); print(s$pval.adjustments)
anderson <- function(W, n, d) {
  rho <- 1 - (2 * d^2 + d + 2) / (6 * d * n)
  w2 <- (d + 2) * (d - 1) * (d - 2) * (2 * d^3 + 6 * d^2 + 3 * d + 2) / (288 * (n * d * rho)^2)
  z <- -n * rho * log(W); f <- d * (d + 1) / 2 - 1
  c(chisq = z, p = pchisq(z, f, lower.tail = FALSE) +
    w2 * (pchisq(z, f + 4, lower.tail = FALSE) - pchisq(z, f, lower.tail = FALSE)))
}
print(anderson(s$sphericity.tests["phase", 1], 10, 2))
print(anderson(s$sphericity.tests["hour", 1], 10, 4))
print(anderson(s$sphericity.tests["phase:hour", 1], 10, 8))
write.csv(OBrienKaiser, row.names = FALSE)

# Reference values for crates/stat-bayes/tests/linear_reference.rs.
# R 4.3 with BayesFactor 0.9.12. mtcars and ToothGrowth are R's datasets.
suppressMessages(library(BayesFactor))
options(digits = 15)
d <- mtcars[, c("mpg", "wt", "hp", "qsec")]
bf <- regressionBF(mpg ~ wt + hp + qsec, data = d, progress = FALSE)
print(extractBF(bf, logbf = TRUE)[, 1:2])
bfw <- regressionBF(mpg ~ wt + hp + qsec, data = d, rscaleCont = 1, progress = FALSE)
print(extractBF(bfw, logbf = TRUE)[, 1:2])
set.seed(2)
post <- posterior(lmBF(mpg ~ wt + hp, data = d), iterations = 200000)
print(colMeans(post[, c("wt", "hp")]))
print(apply(post[, c("wt", "hp")], 2, sd))
print(apply(post[, c("wt", "hp")], 2, quantile, c(0.025, 0.975)))
tg <- ToothGrowth
tg$dose <- factor(tg$dose)
set.seed(3)
a <- anovaBF(len ~ supp * dose, data = tg, progress = FALSE, iterations = 200000)
print(extractBF(a, logbf = TRUE)[, 1:2])
a1 <- anovaBF(len ~ dose, data = tg, progress = FALSE)
print(extractBF(a1, logbf = TRUE)[, 1:2])
# Unbalanced one-way: drop rows.
tu <- tg[-c(1, 2, 3, 31, 45), ]
a2 <- anovaBF(len ~ dose, data = tu, progress = FALSE)
print(extractBF(a2, logbf = TRUE)[, 1:2])
set.seed(4)
pd <- posterior(lmBF(len ~ supp + dose, data = tg), iterations = 200000)
print(colMeans(pd[, grep("supp|dose", colnames(pd))]))
print(apply(pd[, grep("supp|dose", colnames(pd))], 2, sd))
# Mixed continuous + factor.
set.seed(5)
tn <- ToothGrowth
tn$doseN <- tn$dose
m <- lmBF(len ~ supp + doseN, data = tn, progress = FALSE)
print(extractBF(m, logbf = TRUE)[, 1:2])

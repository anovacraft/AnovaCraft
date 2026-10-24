# Cross-validated pruning of a regression tree, for
# cross_validation_matches_rpart_fold_trees (src/tree.rs).
# R 4.3.3, rpart 4.1.23. With fixed folds, each fold's tree is grown by rpart
# and pruned with prune.rpart at the step's representative complexity
# parameter (the geometric mean of consecutive CP values, ten times the first
# CP for the root), which rpart expresses relative to the tree's own root
# risk. The held-out squared errors, summed and divided by the full tree's
# root risk, give each step's cross-validated relative error.
# Writes cart_pruning_reference.txt:
#   x values; y values; fold (1-based) of each row; then one line per
#   cptable row (root first): nsplit, CP, cross-validated relative error.
library(rpart)
set.seed(20261019)
n <- 200
x <- (0:(n - 1)) / n
y <- ifelse(x < 0.5, 0, 3) + ifelse(x > 0.8, 1.5, 0) + rnorm(n)
folds <- (0:(n - 1)) %% 10 + 1
d <- data.frame(x = x, y = y)
ctl <- rpart.control(minsplit = 10, minbucket = 5, cp = 0, xval = 0, maxcompete = 0, maxsurrogate = 0,
                     maxdepth = 4)
fit <- rpart(y ~ x, data = d, method = "anova", control = ctl)
cp <- fit$cptable[, "CP"]
beta <- c(10 * cp[1], sqrt(cp[-1] * cp[-length(cp)]))
root <- sum((y - mean(y))^2)
err <- matrix(0, n, length(cp))
for (k in 1:10) {
  held <- folds == k
  fold_fit <- rpart(y ~ x, data = d[!held, ], method = "anova", control = ctl)
  for (j in seq_along(cp)) {
    err[held, j] <- (y[held] - predict(prune(fold_fit, cp = beta[j]), d[held, ]))^2
  }
}
cv <- colSums(err) / root
out <- file("cart_pruning_reference.txt", "w")
writeLines(paste(format(x, digits = 17), collapse = " "), out)
writeLines(paste(format(y, digits = 17), collapse = " "), out)
writeLines(paste(folds, collapse = " "), out)
for (j in seq_along(cp)) {
  writeLines(paste(fit$cptable[j, "nsplit"], format(cp[j], digits = 17), format(cv[j], digits = 17)), out)
}
close(out)

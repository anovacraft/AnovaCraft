# Reference fits for the MARS tests (crates/stat-predictive/src/mars.rs),
# computed with the R package earth (5.3.3; Milborrow, after Friedman 1991).
# Run from this directory:  Rscript mars_reference.R
# It writes mars_friedman1.csv and mars_categorical.csv (data rounded to six
# decimals, then read back so both programs fit the same numbers) and prints
# the values pasted into the tests.
#
# Settings that make earth follow the plain algorithm of Friedman (1991):
#   fast.k = 0            no "fast MARS" parent heuristics
#   Auto.linpreds = FALSE a knot at a predictor's minimum stays a hinge
#   Adjust.endspan = 1    the same end span for interaction terms
#   minspan, endspan      given explicitly (earth's grid of candidate knots
#                         differs from Friedman's near the low end)
#   pmethod = "backward"  pruning by backward elimination and GCV
suppressMessages(library(earth))
options(digits = 15)

set.seed(20261016)
n <- 200
x <- matrix(runif(n * 10), n, 10)
colnames(x) <- paste0("x", 1:10)
y <- 10 * sin(pi * x[, 1] * x[, 2]) + 20 * (x[, 3] - 0.5)^2 + 10 * x[, 4] + 5 * x[, 5] + rnorm(n)
f1 <- data.frame(round(x, 6), y = round(y, 6))
write.csv(f1, "mars_friedman1.csv", row.names = FALSE)
f1 <- read.csv("mars_friedman1.csv")

set.seed(7)
m <- 150
g <- sample(c("a", "b", "c"), m, replace = TRUE)
u <- runif(m); v <- runif(m); w <- runif(m)
yc <- 3 * pmax(0, u - 0.4) - 2 * pmax(0, 0.6 - v) + ifelse(g == "b", 1.5, ifelse(g == "c", -1, 0)) +
  2 * ifelse(g == "c", pmax(0, u - 0.5), 0) + 0.2 * rnorm(m)
cat_data <- data.frame(u = round(u, 6), v = round(v, 6), w = round(w, 6), g = g, y = round(yc, 6))
write.csv(cat_data, "mars_categorical.csv", row.names = FALSE)
cat_data <- read.csv("mars_categorical.csv", stringsAsFactors = TRUE)

report <- function(label, mod, newdata) {
  cat("====", label, "\n")
  cat("rss", format(mod$rss, digits = 15), "gcv", format(mod$gcv, digits = 15),
      "rsq", format(mod$rsq, digits = 15), "grsq", format(mod$grsq, digits = 15), "\n")
  cat("forward terms", nrow(mod$dirs), "selected", length(mod$selected.terms), "\n")
  print(mod$coefficients)
  cat("predictions", format(predict(mod, newdata = newdata[1:5, ])[, 1], digits = 15), "\n")
  print(evimp(mod, trim = FALSE, sqrt. = FALSE)[, c("nsubsets", "gcv", "rss")])
}

common <- list(fast.k = 0, Auto.linpreds = FALSE, Adjust.endspan = 1, pmethod = "backward", trace = 1)

a1 <- do.call(earth, c(list(y ~ ., data = f1, degree = 1, minspan = 1, endspan = 5, nk = 21), common))
report("friedman1 degree 1 minspan 1 endspan 5 nk 21", a1, f1)

a2 <- do.call(earth, c(list(y ~ ., data = f1, degree = 2, minspan = 1, endspan = 5, nk = 21), common))
report("friedman1 degree 2 minspan 1 endspan 5 nk 21", a2, f1)

# Nested backward elimination (Friedman's Algorithm 3), written here with lm
# on earth's forward-pass basis, as a reference for the subset sequence and
# the variable importance (earth's own pruning uses leaps, which keeps the
# best subset of each size seen during elimination; those need not be
# nested, so its importance counts can differ).
a1f <- do.call(earth, c(list(y ~ ., data = f1, degree = 1, minspan = 1, endspan = 5, nk = 21),
                        modifyList(common, list(pmethod = "none", trace = 0))))
bx <- a1f$bx
nr <- nrow(bx)
rss_of <- function(cols) sum(resid(lm(f1$y ~ bx[, cols, drop = FALSE] - 1))^2)
gcv_of <- function(rss, k, d = 2) rss / (nr * (1 - (k + d * (k - 1) / 2) / nr)^2)
cur <- seq_len(ncol(bx))
seq_terms <- list(); seq_rss <- c()
repeat {
  seq_terms[[length(cur)]] <- cur
  seq_rss[length(cur)] <- rss_of(cur)
  if (length(cur) == 1) break
  drops <- sapply(cur[-1], function(j) rss_of(setdiff(cur, j)))
  cur <- setdiff(cur, cur[-1][which.min(drops)])
}
gcvs <- sapply(seq_along(seq_rss), function(k) gcv_of(seq_rss[k], k))
best <- which.min(gcvs)
cat("==== nested backward elimination on the degree-1 forward basis\n")
cat("rss by size", format(seq_rss, digits = 15), "\n")
cat("selected size", best, "terms", colnames(bx)[seq_terms[[best]]], "\n")
preds <- sub("^h\\(([0-9.]+-)?(x[0-9]+).*$", "\\2", colnames(bx))
imp_n <- c(); imp_g <- c()
for (v in paste0("x", 1:10)) {
  n_sub <- 0; g_sum <- 0
  for (k in 2:best) if (v %in% preds[seq_terms[[k]]]) { n_sub <- n_sub + 1; g_sum <- g_sum + gcvs[k - 1] - gcvs[k] }
  imp_n <- c(imp_n, n_sub); imp_g <- c(imp_g, g_sum)
}
cat("nsubsets", imp_n, "\n")
cat("gcv decrease", format(imp_g, digits = 15), "\n")

a3 <- do.call(earth, c(list(y ~ ., data = f1, degree = 2, minspan = 1, endspan = 5, nk = 21, penalty = 2), common))
report("friedman1 degree 2 penalty 2", a3, f1)

a5 <- do.call(earth, c(list(y ~ ., data = cat_data, degree = 1, minspan = 1, endspan = 5, nk = 21), common))
report("categorical degree 1", a5, cat_data)

a4 <- do.call(earth, c(list(y ~ ., data = cat_data, degree = 2, minspan = 1, endspan = 5, nk = 21), common))
report("categorical degree 2", a4, cat_data)

a6 <- do.call(earth, c(list(y ~ ., data = f1, degree = 1, nk = 21), common))
report("friedman1 degree 1 default spans", a6, f1)

# Friedman's span rules: the values earth prints with trace = 2 for the
# default minspan = 0, endspan = 0.
invisible(earth(y ~ ., data = f1, degree = 1, fast.k = 0, nk = 3, trace = 2))

# ATT matching reference for stat_discrete::treatment_effects: R MatchIt 4.5
# (nearest-neighbour matching on the logit of a logistic propensity score,
# largest scores first) and sandwich 3.1 for the robust standard errors of
# the weighted regression on the matched data (cluster-robust by matched set
# without replacement, HC1 with replacement), as MatchIt's documentation
# recommends. Data: app-model/tests/data/treatment_study.csv (invented).
# Run: Rscript treatment_effects_matchit.R  (writes treatment_effects_matchit.json)
suppressPackageStartupMessages({ library(MatchIt); library(sandwich) })
d <- read.csv("../../../app-model/tests/data/treatment_study.csv", check.names = FALSE)
d$treat <- as.integer(d$Programme == "Enrolled")
d$male <- as.integer(d$Sex == "M")
d$smoker <- as.integer(d$Smoker == "Yes")
d$north <- as.integer(d$Region == "North")
d$south <- as.integer(d$Region == "South")
d$y <- d[["Fitness gain"]]
f <- treat ~ Age + male + Severity + smoker + north + south
num <- function(x) formatC(x, digits = 17, format = "g")
cases <- list()
for (s in list(list(1, FALSE, 0.2), list(2, FALSE, 0.2), list(1, TRUE, 0.2), list(3, TRUE, NULL), list(1, FALSE, NULL))) {
  args <- list(f, data = d, method = "nearest", distance = "glm", link = "linear.logit",
               estimand = "ATT", ratio = s[[1]], replace = s[[2]], m.order = "largest")
  if (!is.null(s[[3]])) args$caliper <- s[[3]]
  m <- do.call(matchit, args)
  md <- match.data(m)
  fit <- lm(y ~ treat, data = md, weights = weights)
  V <- if (s[[2]]) vcovHC(fit, type = "HC1") else vcovCL(fit, cluster = ~subclass, type = "HC1")
  w <- m$weights
  cases[[length(cases) + 1]] <- sprintf(
    '{"ratio": %d, "replace": %s, "caliper": %s, "estimate": %s, "se": %s, "weights": [%s]}',
    s[[1]], tolower(s[[2]]), if (is.null(s[[3]])) "null" else num(s[[3]]),
    num(coef(fit)[["treat"]]), num(sqrt(V["treat", "treat"])), paste(num(w), collapse = ", "))
}
writeLines(paste0('{"cases": [\n', paste(cases, collapse = ",\n"), "\n]}"), "treatment_effects_matchit.json")
cat("wrote", length(cases), "cases\n")

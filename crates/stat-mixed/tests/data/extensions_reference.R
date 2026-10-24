# Reference values for the mixed-model extensions (roadmap §27): weights and
# offsets, cluster-robust (CR2) standard errors, unequal residual variances,
# between-within degrees of freedom, the cumulative logit mixed model and
# shelf life with random batches.
# Run from this folder with `Rscript extensions_reference.R` (R 4.3, lme4
# 1.1-35, lmerTest 3.1, nlme 3.1-164, ordinal 2023.12, clubSandwich 0.5.10,
# jsonlite); it writes extensions_reference.json.
suppressMessages({
  library(lme4); library(lmerTest); library(nlme); library(ordinal)
  library(clubSandwich); library(jsonlite)
})
out <- list()
num <- function(x) unname(as.numeric(x))

# ---- Orthodont (Potthoff & Roy 1964), as in nlme -------------------------
data(Orthodont, package = "nlme")
o <- as.data.frame(Orthodont)
o$Subject <- factor(as.character(o$Subject))
o$Sex <- factor(as.character(o$Sex), levels = c("Male", "Female"))
out$orthodont <- list(
  distance = o$distance, age = o$age, female = num(o$Sex == "Female"),
  subject = as.integer(o$Subject) - 1L, n_subjects = nlevels(o$Subject)
)

# Between-within df (random intercepts).
m1 <- lme(distance ~ age * Sex, random = ~ 1 | Subject, data = o)
tt <- summary(m1)$tTable
out$between_within <- list(df = num(tt[, "DF"]), t = num(tt[, "t-value"]))

# varIdent by sex, REML.
m3 <- lme(distance ~ age * Sex, random = ~ 1 | Subject, weights = varIdent(form = ~ 1 | Sex), data = o)
tt <- summary(m3)$tTable
vs <- m3$modelStruct$varStruct
sd_by_sex <- tapply(m3$sigma / varWeights(vs), attr(vs, "groups"), function(v) v[1])
out$var_ident <- list(
  estimate = num(tt[, "Value"]), std_error = num(tt[, "Std.Error"]),
  sd_male = num(sd_by_sex["Male"]), sd_female = num(sd_by_sex["Female"]),
  subject_sd = as.numeric(VarCorr(m3)[1, "StdDev"]),
  minus_two_ll = -2 * as.numeric(logLik(m3)),
  lr = as.numeric(anova(m1, m3)$L.Ratio[2])
)

# CR2 robust standard errors, clustered by subject.
m4 <- lmer(distance ~ age * Sex + (1 | Subject), data = o)
ct <- coef_test(m4, vcov = "CR2", test = "Satterthwaite")
wt <- Wald_test(m4, constraints = constrain_zero(3:4), vcov = "CR2", test = "HTZ")
out$cr2 <- list(
  std_error = num(ct$SE), df = num(ct$df_Satt),
  htz_f = num(wt$Fstat), htz_df = num(wt$df_denom), htz_p = num(wt$p_val)
)
# CR2 with a random slope.
m5 <- lmer(distance ~ age * Sex + (age | Subject), data = o)
ct5 <- coef_test(m5, vcov = "CR2", test = "Satterthwaite")
out$cr2_slope <- list(std_error = num(ct5$SE), df = num(ct5$df_Satt), estimate = num(fixef(m5)))

# Prior weights in a linear mixed model.
o$w <- 1 + (seq_len(nrow(o)) %% 3)
m6 <- lmer(distance ~ age + (1 | Subject), data = o, weights = w)
out$weighted_lmm <- list(
  w = o$w, estimate = num(fixef(m6)), std_error = num(sqrt(diag(vcov(m6)))),
  subject_var = as.data.frame(VarCorr(m6))$vcov[1], sigma = sigma(m6), reml = REMLcrit(m6)
)

# ---- Poisson GLMM with weights and an offset ------------------------------
set.seed(27)
g <- rep(0:9, each = 8)
x <- round(runif(80, 0, 2), 2)
expo <- round(runif(80, 0.5, 3), 2)
b <- rnorm(10, sd = 0.5)
y <- rpois(80, expo * exp(0.2 + 0.6 * x + b[g + 1]))
w <- 1 + (seq_len(80) %% 2)
d <- data.frame(y, x, expo, w, g = factor(g))
m7 <- glmer(y ~ x + (1 | g) + offset(log(expo)), data = d, family = poisson, weights = w)
out$poisson <- list(
  y = y, x = x, exposure = expo, w = w, g = g,
  estimate = num(fixef(m7)), std_error = num(sqrt(diag(vcov(m7)))),
  g_var = as.data.frame(VarCorr(m7))$vcov[1], minus_two_ll = -2 * as.numeric(logLik(m7))
)

# ---- Wine bitterness (Randall 1989), as in ordinal ------------------------
data(wine, package = "ordinal")
c1 <- clmm(rating ~ temp + contact + (1 | judge), data = wine)
c2 <- clmm(rating ~ temp + contact + (1 | judge), data = wine, nAGQ = 10)
out$wine <- list(
  rating = as.integer(wine$rating) - 1L, warm = num(wine$temp == "warm"),
  contact = num(wine$contact == "yes"), judge = as.integer(wine$judge) - 1L,
  laplace = list(coef = num(coef(c1)), std_error = num(sqrt(diag(vcov(c1)))),
                 judge_var = num(c1$ST$judge)^2, minus_two_ll = -2 * as.numeric(logLik(c1))),
  agq10 = list(coef = num(coef(c2)), judge_var = num(c2$ST$judge)^2,
               minus_two_ll = -2 * as.numeric(logLik(c2)))
)
# Correlated random intercept and slope (on contact) by judge, Laplace.
c3 <- clmm(rating ~ temp + contact + (1 + contact | judge), data = wine)
vc3 <- VarCorr(c3)$judge
out$wine_slope <- list(coef = num(coef(c3)), minus_two_ll = -2 * as.numeric(logLik(c3)),
                       var = num(diag(vc3)))

# ---- Stability study with six batches (synthetic) -------------------------
set.seed(51)
months <- c(0, 3, 6, 9, 12, 18, 24)
batch <- rep(0:5, each = length(months))
t <- rep(months, 6)
b0 <- rnorm(6, sd = 0.6); b1 <- rnorm(6, sd = 0.03)
assay <- round(100.5 + b0[batch + 1] + (-0.12 + b1[batch + 1]) * t + rnorm(42, sd = 0.35), 1)
s <- data.frame(assay, t, batch = factor(batch))
ms <- lmer(assay ~ t + (t | batch), data = s)
V <- vcov(ms)
G <- as.matrix(VarCorr(ms)$batch)
A <- ms@vcov_varpar           # covariance of (theta, sigma)
J <- ms@Jac_list              # d vcov(beta) / d (theta, sigma)
# dG / d(theta, sigma) by central differences of sigma^2 * T T'.
Gof <- function(par) {
  th <- par[1:3]; sg <- par[4]
  Tm <- matrix(c(th[1], th[2], 0, th[3]), 2, 2)
  sg^2 * Tm %*% t(Tm)
}
par <- c(getME(ms, "theta"), sigma(ms))
dG <- lapply(1:4, function(i) {
  h <- 1e-5 * max(abs(par[i]), 0.05); up <- par; dn <- par; up[i] <- up[i] + h; dn[i] <- dn[i] - h
  (Gof(up) - Gof(dn)) / (2 * h)
})
bound <- function(tt, future, lower = TRUE) {
  x <- c(1, tt)
  v <- drop(t(x) %*% V %*% x)
  g <- sapply(1:4, function(i) drop(t(x) %*% J[[i]] %*% x))
  if (future) {
    v <- v + drop(t(x) %*% G %*% x)
    g <- g + sapply(1:4, function(i) drop(t(x) %*% dG[[i]] %*% x))
  }
  df <- 2 * v^2 / drop(t(g) %*% A %*% g)
  est <- sum(fixef(ms) * x)
  c(est = est, bound = est - qt(0.95, df) * sqrt(v), df = df, var = v)
}
at <- c(12, 24, 36)
mean_b <- sapply(at, bound, future = FALSE)
fut_b <- sapply(at, bound, future = TRUE)
life <- function(future) uniroot(function(tt) bound(tt, future)["bound"] - 95, c(0, 200), tol = 1e-10)$root
out$stability <- list(
  assay = assay, time = t, batch = batch,
  estimate = num(fixef(ms)), random_cov = num(G), at = at,
  mean_bound = num(mean_b["bound", ]), mean_df = num(mean_b["df", ]),
  future_bound = num(fut_b["bound", ]), future_df = num(fut_b["df", ]),
  mean_life = life(FALSE), future_life = life(TRUE)
)

writeLines(toJSON(out, digits = NA, auto_unbox = TRUE), "extensions_reference.json")

# Reference values for crates/stat-timeseries/src/trend.rs from R 4.3 (stats
# lm, predict, nls with the SSlogis self-starting model) used as black boxes.
# Time index t = 1..n. Accuracy measures are computed directly from their
# definitions.  Run from this directory:  Rscript trend_reference.R
suppressMessages(library(jsonlite))

acc <- function(y, f) {
  e <- y - f
  list(me = mean(e), mad = mean(abs(e)), msd = mean(e^2), rmse = sqrt(mean(e^2)), mape = 100 * mean(abs(e / y)))
}

h <- 5
ls_fit <- function(name, y, model) {
  n <- length(y); t <- seq_len(n); new <- data.frame(t = n + seq_len(h))
  if (model == "linear") fit <- lm(y ~ t)
  if (model == "quadratic") fit <- lm(y ~ t + I(t^2))
  if (model == "exponential") fit <- lm(log(y) ~ t)
  pr <- predict(fit, new, interval = "prediction", level = 0.95)
  co <- unname(coef(fit)); se <- unname(sqrt(diag(vcov(fit))))
  fitted <- unname(fitted(fit))
  if (model == "exponential") { co <- exp(co); fitted <- exp(fitted); pr <- exp(pr) }
  list(series = name, model = model, coefficients = co, std_errors = se, sigma = summary(fit)$sigma,
       fitted = fitted, accuracy = acc(y, fitted),
       forecast = unname(pr[, "fit"]), lower = unname(pr[, "lwr"]), upper = unname(pr[, "upr"]))
}

lake <- as.numeric(LakeHuron)
air <- as.numeric(AirPassengers)
nile <- as.numeric(Nile)
pop <- as.numeric(uspop)
# A constructed S-shaped series with deterministic disturbances.
s_curve <- 100 / (1 + 40 * exp(-0.25 * (1:40))) + 1.5 * sin(1:40)

cases <- list(
  ls_fit("lake_huron", lake, "linear"),
  ls_fit("lake_huron", lake, "quadratic"),
  ls_fit("air", air, "linear"),
  ls_fit("air", air, "quadratic"),
  ls_fit("air", air, "exponential"),
  ls_fit("uspop", pop, "exponential")
)

logis <- list()
for (s in list(list("uspop", pop), list("s_curve", s_curve))) {
  y <- s[[2]]; t <- seq_along(y)
  fit <- nls(y ~ SSlogis(t, Asym, xmid, scal))
  p <- coef(fit)
  # y = c / (1 + a exp(-b t)) with c = Asym, b = 1/scal, a = exp(xmid/scal)
  logis[[length(logis) + 1]] <- list(
    series = s[[1]], asym = unname(p["Asym"]), xmid = unname(p["xmid"]), scal = unname(p["scal"]),
    sse = sum(resid(fit)^2), fitted = unname(fitted(fit)), accuracy = acc(y, unname(fitted(fit))),
    forecast = unname(predict(fit, data.frame(t = length(y) + seq_len(h))))
  )
}

ma <- list()
for (k in c(3, 12)) {
  y <- air; n <- length(y)
  f <- stats::filter(y, rep(1 / k, k), sides = 1)  # f[t] = mean(y[t-k+1..t])
  one_step <- as.numeric(f[k:(n - 1)])              # forecasts of y[k+1..n]
  ma[[length(ma) + 1]] <- list(span = k, fitted = one_step, forecast = mean(y[(n - k + 1):n]),
                               accuracy = acc(y[(k + 1):n], one_step))
}

out <- list(series = list(lake_huron = lake, air = air, nile = nile, uspop = pop, s_curve = s_curve),
            horizon = h, least_squares = cases, logistic = logis, moving_average = ma)
writeLines(toJSON(out, digits = NA, auto_unbox = TRUE, pretty = TRUE), "trend_reference.json")

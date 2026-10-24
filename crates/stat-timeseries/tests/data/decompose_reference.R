# Reference values for crates/stat-timeseries (autocorrelation, transform,
# decompose) computed with R's stats and forecast packages, used only as
# black-box reference software. Run from this directory:
#   Rscript decompose_reference.R
# which writes decompose_reference.json. The series are R's `datasets`.
suppressPackageStartupMessages({ library(jsonlite); library(forecast) })
v <- function(x) as.numeric(x)
out <- list()
out$series <- list(
  lh = v(lh), LakeHuron = v(LakeHuron), lynx = v(lynx), AirPassengers = v(AirPassengers),
  nottem = v(nottem), co2 = v(co2), UKgas = v(UKgas), USAccDeaths = v(USAccDeaths),
  mdeaths = v(mdeaths), fdeaths = v(fdeaths), WWWusage = v(WWWusage)
)

# Autocorrelation: acf / pacf with the default lag.max.
acf_case <- function(name, x, log = FALSE) {
  y <- if (log) log(x) else x
  a <- acf(y, plot = FALSE); p <- pacf(y, plot = FALSE)
  list(series = name, log = log, max_lag = length(a$acf) - 1, acf = v(a$acf), pacf = v(p$acf))
}
out$acf <- list(acf_case("lh", lh), acf_case("LakeHuron", LakeHuron),
                acf_case("lynx", lynx, TRUE), acf_case("AirPassengers", AirPassengers))

# Portmanteau tests at several lags with a fitted-parameter adjustment.
box_case <- function(name, x, lags, fitdf, type) {
  r <- lapply(lags, function(k) { b <- Box.test(x, lag = k, type = type, fitdf = fitdf)
    list(lag = k, statistic = v(b$statistic), df = v(b$parameter), p_value = b$p.value) })
  list(series = name, fitdf = fitdf, type = type, tests = r)
}
out$box <- list(box_case("lh", lh, c(5, 10, 20), 0, "Ljung-Box"),
                box_case("lh", lh, c(5, 10, 20), 2, "Ljung-Box"),
                box_case("LakeHuron", LakeHuron, c(6, 12), 1, "Box-Pierce"),
                box_case("WWWusage", WWWusage, c(10, 24), 3, "Ljung-Box"))

# Yule-Walker AR fits, order by AIC unless fixed.
ar_case <- function(name, x, log = FALSE, order = NULL) {
  y <- if (log) log(x) else x
  a <- if (is.null(order)) ar.yw(y) else ar.yw(y, aic = FALSE, order.max = order)
  list(series = name, log = log, order = a$order, fixed = !is.null(order), ar = v(a$ar),
       mean = a$x.mean, var_pred = v(a$var.pred), aic = v(a$aic))
}
out$ar <- list(ar_case("lh", lh), ar_case("LakeHuron", LakeHuron), ar_case("lynx", lynx, TRUE),
               ar_case("lh", lh, order = 3))

# Cross-correlation: ccf(x, y)$acf at lag k estimates cor(x[t+k], y[t]).
cc <- ccf(mdeaths, fdeaths, plot = FALSE)
out$ccf <- list(x = "mdeaths", y = "fdeaths", lags = round(v(cc$lag) * 12), values = v(cc$acf))
# Prewhitened: AR fitted to x by ar.yw (order by AIC), both series filtered by
# it, then cross-correlated. y is the first 98 values of log(lynx).
x <- v(LakeHuron); y <- v(log(lynx))[1:length(x)]
fx <- ar.yw(x)
pw <- function(s) v(na.omit(stats::filter(s, c(1, -fx$ar), sides = 1)))
cp <- ccf(pw(x), pw(y), plot = FALSE, lag.max = 12)
out$ccf_prewhitened <- list(x = "LakeHuron", y = "lynx_log_head", order = fx$order,
                            lags = v(cp$lag), values = v(cp$acf))

# Differencing.
ap <- AirPassengers
out$diff <- list(
  list(d = 1, D = 0, m = 12, values = v(diff(ap))),
  list(d = 2, D = 0, m = 12, values = v(diff(ap, differences = 2))),
  list(d = 1, D = 1, m = 12, values = v(diff(diff(ap, lag = 12)))),
  list(d = 0, D = 2, m = 12, values = v(diff(ap, lag = 12, differences = 2))))

# Box-Cox lambda by Guerrero's method.
out$guerrero <- list(
  list(series = "AirPassengers", period = 12, lambda = BoxCox.lambda(AirPassengers, method = "guerrero")),
  list(series = "UKgas", period = 4, lambda = BoxCox.lambda(UKgas, method = "guerrero")),
  list(series = "USAccDeaths", period = 12, lambda = BoxCox.lambda(USAccDeaths, method = "guerrero")),
  list(series = "co2", period = 12, lambda = BoxCox.lambda(co2, method = "guerrero")),
  list(series = "lynx", period = 1, lambda = BoxCox.lambda(lynx, method = "guerrero")))

# Classical decomposition.
dc <- function(name, x, type) { d <- decompose(x, type = type)
  list(series = name, type = type, period = frequency(x), trend = v(d$trend), seasonal = v(d$seasonal),
       random = v(d$random), figure = v(d$figure)) }
out$classical <- list(dc("co2", co2, "additive"), dc("AirPassengers", AirPassengers, "multiplicative"),
                      dc("UKgas", UKgas, "additive"))

# STL. Non-robust fits, and robust fits limited to one or two robustness
# iterations: beyond those R's robustness scale is not always 6 * median|R|
# (see stl_robust_reference.py, which provides the robust references).
st <- function(name, x, s.window, robust, log = FALSE, t.window = NULL, outer = NULL) {
  y <- if (log) log(x) else x
  args <- list(y, s.window = s.window, robust = robust)
  if (!is.null(t.window)) args$t.window <- t.window
  if (!is.null(outer)) args$outer <- outer
  s <- do.call(stl, args)
  r <- s$time.series[, "remainder"]
  list(series = name, log = log, period = frequency(x), s_window = if (is.character(s.window)) NULL else s.window,
       t_window = t.window, robust = robust, outer = outer, seasonal = v(s$time.series[, "seasonal"]),
       trend = v(s$time.series[, "trend"]), remainder = v(r),
       weights = v(s$weights), windows = v(s$win),
       strength = max(0, 1 - var(r) / var(r + s$time.series[, "seasonal"])))
}
out$stl <- list(st("nottem", nottem, "periodic", FALSE), st("nottem", nottem, 7, FALSE),
                st("co2", co2, "periodic", FALSE), st("co2", co2, 13, TRUE, outer = 1),
                st("co2", co2, 13, TRUE, outer = 2),
                st("UKgas", UKgas, 9, FALSE, log = TRUE),
                st("UKgas", UKgas, "periodic", TRUE, log = TRUE, outer = 2),
                st("AirPassengers", AirPassengers, "periodic", FALSE, log = TRUE),
                st("AirPassengers", AirPassengers, 11, TRUE, log = TRUE, t.window = 15, outer = 1))
writeLines(toJSON(out, auto_unbox = TRUE, digits = NA, null = "null", na = "null"), "decompose_reference.json")

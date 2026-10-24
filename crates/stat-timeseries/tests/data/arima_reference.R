# Reference values for crates/stat-timeseries/tests/arima_reference.rs.
#
# R's stats::arima(method = "ML") and the forecast package (Arima,
# auto.arima) used as black-box reference software on classic series from
# R's datasets package. Run from this directory:
#
#   Rscript arima_reference.R
#
# which writes arima_reference.json next to this script.
suppressPackageStartupMessages({
  library(forecast)
  library(jsonlite)
})

h <- 12

# R starts the differenced part of the state with variance kappa (default
# 1e6, in units of sigma^2) instead of the exact diffuse limit. kappa = 1e10
# brings R to within about 1e-7 of that limit (larger values lose precision),
# so the comparisons use it; the default-kappa likelihood is recorded too.
kappa <- 1e10

case <- function(y, order, seasonal = c(0, 0, 0), period = 1, mean = FALSE,
                 drift = FALSE, xreg = NULL, newxreg = NULL) {
  y <- as.numeric(y)
  n <- length(y)
  ts_y <- ts(y, frequency = period)
  if (drift) {
    # Drift as forecast::Arima defines it: a regressor 1..n. stats::arima is
    # called directly because forecast::forecast scales its standard errors
    # by a degrees-of-freedom-corrected sigma^2.
    xreg <- seq_len(n)
    newxreg <- n + seq_len(h)
  }
  sord <- list(order = seasonal, period = period)
  fit <- arima(ts_y, order = order, seasonal = sord, include.mean = mean, xreg = xreg,
               method = "ML", kappa = kappa)
  cf <- coef(fit)
  # Standard errors from the default fit: with kappa = 1e10 R's
  # finite-difference Hessian becomes noisy.
  fit_default <- arima(ts_y, order = order, seasonal = sord, include.mean = mean, xreg = xreg,
                       method = "ML")
  fixed <- arima(ts_y, order = order, seasonal = sord, include.mean = mean, xreg = xreg,
                 method = "ML", fixed = cf, transform.pars = FALSE, kappa = kappa)
  fixed_default <- arima(ts_y, order = order, seasonal = sord, include.mean = mean, xreg = xreg,
                         method = "ML", fixed = cf, transform.pars = FALSE)
  p <- predict(fit, n.ahead = h, newxreg = newxreg)
  list(
    y = y, order = order, seasonal = seasonal, period = period, mean = mean, drift = drift,
    xreg = if (is.null(xreg) || drift) list() else list(as.numeric(xreg)),
    newxreg = if (is.null(newxreg) || drift) list() else list(as.numeric(newxreg)),
    coef = unname(cf), se = unname(sqrt(diag(fit_default$var.coef))),
    sigma2 = fit$sigma2, loglik = fit$loglik, aic = fit$aic,
    fixed_loglik = fixed$loglik, fixed_sigma2 = fixed$sigma2,
    fixed_loglik_default_kappa = fixed_default$loglik,
    residuals = as.numeric(residuals(fit)),
    horizon = h, pred = as.numeric(p$pred), pred_se = as.numeric(p$se)
  )
}

lh_trend <- as.numeric(time(LakeHuron) - 1920)
cases <- list(
  airline = case(log(AirPassengers), c(0, 1, 1), c(0, 1, 1), 12),
  lynx_ar2 = case(lynx, c(2, 0, 0), mean = TRUE),
  log10_lynx_ar2 = case(log10(lynx), c(2, 0, 0), mean = TRUE),
  lake_huron_ar2 = case(LakeHuron, c(2, 0, 0), mean = TRUE),
  lake_huron_trend = case(LakeHuron, c(2, 0, 0), mean = TRUE, xreg = lh_trend,
                          newxreg = max(lh_trend) + seq_len(h)),
  lake_huron_arma11 = case(LakeHuron, c(1, 0, 1), mean = TRUE),
  wwwusage = case(WWWusage, c(1, 1, 1)),
  nile_ma1 = case(Nile, c(0, 1, 1)),
  nile_ma2_mean = case(Nile, c(0, 0, 2), mean = TRUE),
  usaccdeaths = case(USAccDeaths, c(1, 1, 1), c(0, 1, 1), 12),
  ldeaths_sar = case(ldeaths, c(1, 0, 0), c(1, 0, 0), 12, mean = TRUE),
  airline_drift = case(log(AirPassengers), c(1, 0, 1), c(0, 1, 1), 12, drift = TRUE),
  lh_ar1 = case(lh, c(1, 0, 0), mean = TRUE)
)

auto <- function(y, period, d, D) {
  y <- ts(as.numeric(y), frequency = period)
  fit <- auto.arima(y, d = d, D = D, stepwise = TRUE, approximation = FALSE)
  free <- auto.arima(y, stepwise = TRUE, approximation = FALSE)
  o <- arimaorder(fit)
  of <- arimaorder(free)
  pad <- function(o) if (length(o) == 3) c(o, 0, 0, 0) else o[1:6]
  list(
    y = as.numeric(y), period = period, d = d, D = D,
    order = unname(pad(o)), constant = length(grep("intercept|drift", names(coef(fit)))) > 0,
    aicc = fit$aicc,
    free_order = unname(pad(of)), free_constant = length(grep("intercept|drift", names(coef(free)))) > 0,
    free_d = unname(of[2]), free_D = if (length(of) > 3) unname(of[5]) else 0
  )
}

autos <- list(
  wwwusage = auto(WWWusage, 1, 1, 0),
  log_air_passengers = auto(log(AirPassengers), 12, 1, 1),
  usaccdeaths = auto(USAccDeaths, 12, 1, 1),
  lh = auto(lh, 1, 0, 0),
  lake_huron = auto(LakeHuron, 1, 1, 0),
  lake_huron_level = auto(LakeHuron, 1, 0, 0),
  nottem = auto(nottem, 12, 0, 1)
)

out <- list(
  generated_by = paste("R", getRversion(), "forecast", packageVersion("forecast")),
  cases = cases,
  auto = autos
)
here <- tryCatch(dirname(sys.frame(1)$ofile), error = function(e) ".")
writeLines(toJSON(out, auto_unbox = TRUE, digits = NA), file.path(here, "arima_reference.json"))

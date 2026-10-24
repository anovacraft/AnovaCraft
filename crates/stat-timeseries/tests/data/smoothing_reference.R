# Reference values for crates/stat-timeseries/src/smoothing (exponential
# smoothing in innovations state-space form). R's forecast package is used as
# black-box reference software only.
#
# Run from this directory:  Rscript smoothing_reference.R
# Writes smoothing_reference.json.
#
# Conventions of the reference software, as observed from its output (and
# converted in the Rust tests):
#   * `loglik` is -0.5 * (n * log(SSE) + 2 * sum(log|mu_t|)), i.e. without the
#     Gaussian constants; SSE is the sum of squared innovations.
#   * `sigma2` is SSE / (n - p) with p the number of estimated parameters
#     (smoothing parameters and initial states); AIC counts p + 1.
#   * `initstate` lists level, trend and then the seasonal states s_0, s_-1,
#     ..., s_{-m+1} (the last one is the seasonal used for the first
#     observation).
suppressMessages({
  library(forecast)
  library(jsonlite)
})

model_case <- function(name, y, model, damped, restrict = TRUE, h = NULL) {
  m <- frequency(y)
  if (is.null(h)) h <- if (m > 1) 2 * m else 10
  fit <- ets(y, model = model, damped = damped, restrict = restrict)
  fc <- forecast(fit, h = h, level = c(80, 95))
  par <- fit$par
  get <- function(k) if (k %in% names(par)) unname(par[[k]]) else NULL
  init <- fit$states[1, ]
  ns <- if (m > 1 && substr(model, 3, 3) != "N") m else 0
  list(
    name = name,
    series = as.numeric(y),
    period = m,
    error = substr(model, 1, 1),
    trend = if (substr(model, 2, 2) == "N") "N" else if (damped) "Ad" else "A",
    season = substr(model, 3, 3),
    method = fit$method,
    alpha = get("alpha"), beta = get("beta"), gamma = get("gamma"), phi = get("phi"),
    level = unname(init[["l"]]),
    slope = if ("b" %in% names(init)) unname(init[["b"]]) else NULL,
    seasonal = if (ns > 0) unname(init[paste0("s", 1:m)]) else list(),
    n_par = length(par),
    loglik = fit$loglik, aic = fit$aic, aicc = fit$aicc, bic = fit$bic,
    sigma2 = fit$sigma2,
    fitted = as.numeric(fitted(fit)),
    residuals = as.numeric(residuals(fit)),
    horizon = h,
    mean = as.numeric(fc$mean),
    lower80 = as.numeric(fc$lower[, 1]), upper80 = as.numeric(fc$upper[, 1]),
    lower95 = as.numeric(fc$lower[, 2]), upper95 = as.numeric(fc$upper[, 2])
  )
}

cases <- list(
  model_case("USAccDeaths AAA", USAccDeaths, "AAA", FALSE),
  model_case("USAccDeaths ANA", USAccDeaths, "ANA", FALSE),
  model_case("AirPassengers MAM", AirPassengers, "MAM", FALSE),
  model_case("AirPassengers AAM", AirPassengers, "AAM", FALSE, restrict = FALSE),
  model_case("WWWusage AAdN", WWWusage, "AAN", TRUE),
  model_case("WWWusage MAdN", WWWusage, "MAN", TRUE),
  model_case("ldeaths MAdA", ldeaths, "MAA", TRUE),
  model_case("Nile ANN", Nile, "ANN", FALSE),
  model_case("Nile MNN", Nile, "MNN", FALSE),
  model_case("LakeHuron AAN", LakeHuron, "AAN", FALSE),
  model_case("UKgas MAM", UKgas, "MAM", FALSE),
  model_case("nottem AAA", nottem, "AAA", FALSE),
  model_case("co2 AAdA", co2, "AAA", TRUE)
)

auto_case <- function(name, y) {
  fit <- ets(y)
  list(name = name, series = as.numeric(y), period = frequency(y),
       method = fit$method, aicc = fit$aicc, n_par = length(fit$par))
}
autos <- list(
  auto_case("USAccDeaths", USAccDeaths),
  auto_case("WWWusage", WWWusage),
  auto_case("Nile", Nile),
  auto_case("ldeaths", ldeaths),
  auto_case("UKgas", UKgas),
  auto_case("AirPassengers", AirPassengers)
)

out <- list(
  source = paste("R", getRversion(), "forecast", as.character(packageVersion("forecast"))),
  cases = cases,
  auto = autos
)
writeLines(toJSON(out, digits = I(17), auto_unbox = TRUE, null = "null"), "smoothing_reference.json")

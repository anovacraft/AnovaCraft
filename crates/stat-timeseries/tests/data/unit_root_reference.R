# Reference values for the KPSS and augmented Dickey-Fuller tests in
# crates/stat-timeseries/src/unit_root.rs, computed with R 4.3 packages
# tseries (kpss.test) and urca (ur.kpss, ur.df) used as black boxes.
# Writes unit_root_reference.json next to this script; the statsmodels
# p-values and critical values are added by unit_root_reference.py, which
# reads the series from that file.
#
# Run from this directory:  Rscript unit_root_reference.R && python3 unit_root_reference.py
suppressMessages({
  library(tseries)
  library(urca)
  library(jsonlite)
})

series <- list(
  log_lynx = as.numeric(log(lynx)),
  nile = as.numeric(Nile),
  lake_huron = as.numeric(LakeHuron),
  log_air = as.numeric(log(AirPassengers)),
  sunspot_year = as.numeric(sunspot.year),
  www_usage = as.numeric(WWWusage)
)

kpss_cases <- list()
adf_cases <- list()
for (name in names(series)) {
  y <- series[[name]]
  for (null in c("Level", "Trend")) {
    for (lshort in c(TRUE, FALSE)) {
      k <- suppressWarnings(kpss.test(y, null = null, lshort = lshort))
      u <- ur.kpss(y, type = if (null == "Level") "mu" else "tau", lags = if (lshort) "short" else "long")
      kpss_cases[[length(kpss_cases) + 1]] <- list(
        series = name, null = null, short = lshort,
        statistic = unname(k$statistic), lag = unname(k$parameter), p_value = k$p.value,
        urca_statistic = u@teststat[1], urca_lag = u@lag
      )
    }
  }
  # ur.df: type none/drift/trend; Fixed lags 0, 1, 4; AIC and BIC from a
  # maximum of 8 lags (urca selects among 1..max over the common sample).
  for (type in c("none", "drift", "trend")) {
    specs <- list(list("Fixed", 0), list("Fixed", 1), list("Fixed", 4), list("AIC", 8), list("BIC", 8))
    for (s in specs) {
      d <- ur.df(y, type = type, lags = s[[2]], selectlags = s[[1]])
      reg <- d@testreg
      chosen <- sum(grepl("^z.diff.lag", rownames(reg$coefficients)))
      gamma <- reg$coefficients["z.lag.1", ]
      adf_cases[[length(adf_cases) + 1]] <- list(
        series = name, type = type, selection = s[[1]], max_lag = s[[2]],
        lag = chosen, statistic = unname(d@teststat[1]), nobs = length(reg$residuals),
        gamma = unname(gamma[1]), gamma_se = unname(gamma[2])
      )
    }
  }
}

out <- list(series = series, kpss = kpss_cases, adf = adf_cases)
writeLines(toJSON(out, digits = NA, auto_unbox = TRUE, pretty = TRUE), "unit_root_reference.json")

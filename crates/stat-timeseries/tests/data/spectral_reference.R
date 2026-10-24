# Reference values for crates/stat-timeseries/src/spectral.rs from R 4.3
# stats::spec.pgram (fast = FALSE, so no zero padding), used as a black box.
# R reports frequencies in cycles per unit time (frequency(x) per
# observation), so the series are passed with frequency 1 and the test
# rescales nothing; ldeaths is also run with its monthly frequency to check
# the documented conversion.  Run from this directory:
#   Rscript spectral_reference.R
suppressMessages(library(jsonlite))

series <- list(
  log_lynx = as.numeric(log(lynx)),
  sunspot_year = as.numeric(sunspot.year),
  ldeaths = as.numeric(ldeaths)
)

specs <- list(
  list(taper = 0.1, detrend = TRUE, demean = FALSE, spans = NULL),
  list(taper = 0, detrend = FALSE, demean = TRUE, spans = NULL),
  list(taper = 0.1, detrend = TRUE, demean = FALSE, spans = c(3)),
  list(taper = 0.25, detrend = FALSE, demean = TRUE, spans = c(3, 5)),
  list(taper = 0, detrend = FALSE, demean = FALSE, spans = c(4, 6, 8))
)

cases <- list()
for (name in names(series)) {
  y <- series[[name]]
  for (s in specs) {
    sp <- spec.pgram(ts(y), taper = s$taper, detrend = s$detrend, demean = s$demean,
                     spans = s$spans, fast = FALSE, plot = FALSE)
    cases[[length(cases) + 1]] <- list(
      series = name, taper = s$taper,
      detrend = if (s$detrend) "linear" else if (s$demean) "mean" else "none",
      spans = I(if (is.null(s$spans)) integer(0) else as.integer(s$spans)),
      freq = sp$freq, spec = as.numeric(sp$spec), df = sp$df, bandwidth = sp$bandwidth
    )
  }
}
# Monthly frequency: frequencies in cycles per year.
sp12 <- spec.pgram(ldeaths, taper = 0.1, spans = c(3, 3), fast = FALSE, plot = FALSE)
monthly <- list(freq = sp12$freq, spec = as.numeric(sp12$spec), df = sp12$df, bandwidth = sp12$bandwidth)

out <- list(series = series, cases = cases, ldeaths_monthly = monthly,
            chisq = list(df = cases[[3]]$df, q975 = qchisq(0.975, cases[[3]]$df), q025 = qchisq(0.025, cases[[3]]$df)))
writeLines(toJSON(out, digits = NA, auto_unbox = TRUE, pretty = TRUE), "spectral_reference.json")

# Reference values for crates/stat-timeseries/tests/transfer_reference.rs.
#
# Transfer-function–noise models y = v(x) + N with v = omega(B) B^b / delta(B)
# applied to the centred input (pre-sample values zero; Box, Jenkins, Reinsel
# & Ljung 2015, ch. 12) and ARIMA noise N. R has no rational transfer
# functions in its base packages, so the maximum likelihood estimate is found
# here by profiling: for given transfer weights the filtered input is
# computed by an explicit recursion, stats::arima(method = "ML") fits the
# noise model to y - v (concentrating out the ARMA coefficients, mean or
# drift and sigma^2), and optim() maximises that profile log-likelihood over
# the transfer weights. Run from this directory:
#
#   Rscript transfer_reference.R
#
# which writes transfer_reference.json next to this script.
suppressPackageStartupMessages(library(jsonlite))

kappa <- 1e10   # near the exact diffuse limit; see arima_reference.R

filt <- function(x, b, omega, delta) {
  xc <- x - mean(x)
  n <- length(xc)
  v <- numeric(n)
  for (t in seq_len(n)) {
    s <- 0
    for (i in seq_along(delta)) if (t - i >= 1) s <- s + delta[i] * v[t - i]
    for (j in seq_along(omega)) {
      lag <- b + j - 1
      if (t - lag >= 1) s <- s + (if (j == 1) 1 else -1) * omega[j] * xc[t - lag]
    }
    v[t] <- s
  }
  v
}

noise_fit <- function(y, v, order, mean, drift, fixed = NULL) {
  n <- length(y)
  xreg <- if (drift) seq_len(n) else NULL
  arima(y - v, order = order, include.mean = mean, xreg = xreg, method = "ML",
        kappa = kappa, fixed = fixed, transform.pars = is.null(fixed),
        optim.control = list(maxit = 1000, reltol = 1e-12))
}

case <- function(y, x, b, s, r, order, mean = FALSE, drift = FALSE, start, horizon = 0,
                 x_future = numeric(0)) {
  y <- as.numeric(y)
  x <- as.numeric(x)
  ns <- s + 1
  prof <- function(p) {
    d <- if (r > 0) p[ns + seq_len(r)] else numeric(0)
    if (r > 0 && any(Mod(polyroot(c(1, -d))) <= 1)) return(1e10)
    f <- try(noise_fit(y, filt(x, b, p[seq_len(ns)], d), order, mean, drift), silent = TRUE)
    if (inherits(f, "try-error")) 1e10 else -f$loglik
  }
  o <- optim(start, prof, method = "Nelder-Mead", control = list(maxit = 5000, reltol = 1e-14))
  o <- optim(o$par, prof, method = "BFGS", control = list(maxit = 1000, reltol = 1e-15, ndeps = rep(1e-5, length(start))))
  omega <- o$par[seq_len(ns)]
  delta <- if (r > 0) o$par[ns + seq_len(r)] else numeric(0)
  v <- filt(x, b, omega, delta)
  # Final noise fit called here, so predict() finds its regressor and series.
  xreg <- if (drift) seq_len(length(y)) else NULL
  yv <- y - v
  fit <- arima(yv, order = order, include.mean = mean, xreg = xreg, method = "ML",
               kappa = kappa, optim.control = list(maxit = 1000, reltol = 1e-12))
  out <- list(
    y = y, x = x, delay = b, numerator = s, denominator = r, order = order,
    mean = mean, drift = drift,
    omega = omega, delta = delta, noise_coef = unname(coef(fit)),
    sigma2 = fit$sigma2, loglik = fit$loglik, effect = v
  )
  if (horizon > 0) {
    # Future filtered input from the extended centred input (same mean).
    xm <- mean(x)
    need <- max(horizon - b, 0)
    ext <- c(x - xm, x_future[seq_len(need)] - xm, rep(0, horizon - need))
    # ext is already centred, so filter it without re-centring.
    vf <- local({
      n <- length(ext); vv <- numeric(n)
      for (t in seq_len(n)) {
        s2 <- 0
        for (i in seq_along(delta)) if (t - i >= 1) s2 <- s2 + delta[i] * vv[t - i]
        for (j in seq_along(omega)) {
          lag <- b + j - 1
          if (t - lag >= 1) s2 <- s2 + (if (j == 1) 1 else -1) * omega[j] * ext[t - lag]
        }
        vv[t] <- s2
      }
      vv
    })
    n <- length(y)
    newxreg <- if (drift) n + seq_len(horizon) else NULL
    p <- predict(fit, n.ahead = horizon, newxreg = newxreg)
    out$horizon <- horizon
    out$x_future <- x_future
    out$pred <- as.numeric(p$pred) + vf[n + seq_len(horizon)]
    out$pred_se <- as.numeric(p$se)
  }
  out
}

set.seed(20261017)
n <- 160
xs <- as.numeric(arima.sim(list(ar = 0.7), n)) + 5
noise <- as.numeric(arima.sim(list(ar = 0.5), n))
v_true <- filt(xs, 2, c(1.2, -0.6), 0.5)
ys <- 10 + v_true + noise

cases <- list(
  # Box-Jenkins Series M (R's BJsales with the leading indicator):
  # (1 - B) Y = omega0 / (1 - delta B) (1 - B) X_{t-3} + (1 - theta B) a.
  series_m = case(BJsales[1:140], BJsales.lead[1:140], b = 3, s = 0, r = 1, order = c(0, 1, 1),
                  start = c(4.8, 0.7), horizon = 10, x_future = BJsales.lead[141:150]),
  series_m_drift = case(BJsales, BJsales.lead, b = 3, s = 0, r = 1, order = c(0, 1, 1),
                        drift = TRUE, start = c(4.8, 0.7)),
  simulated = case(ys, xs, b = 2, s = 1, r = 1, order = c(1, 0, 0), mean = TRUE,
                   start = c(1, -0.5, 0.4), horizon = 6, x_future = rep(5, 6)),
  distributed_lag = case(ys, xs, b = 2, s = 2, r = 0, order = c(1, 0, 0), mean = TRUE,
                         start = c(1, -0.5, -0.3))
)

out <- list(generated_by = R.version.string, cases = cases)
writeLines(toJSON(out, digits = NA, auto_unbox = TRUE), "transfer_reference.json")

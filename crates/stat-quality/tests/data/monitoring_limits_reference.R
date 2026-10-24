# Independent reference values for the environmental monitoring limits.
# Run from this directory: Rscript monitoring_limits_reference.R  (writes monitoring_limits_reference.json)
#
# Maximum-likelihood fits with R 4.3: MASS::fitdistr (Poisson, negative binomial,
# gamma, lognormal, Weibull), pscl::zeroinfl (zero-inflated Poisson and negative
# binomial, intercept-only), and optim (BFGS) on the beta-binomial likelihood
# written out from its definition. Quantiles with qpois, qnbinom, qbinom and the
# continuous q functions; zero-inflated and beta-binomial quantiles by summing
# their probabilities until the target is reached.
suppressMessages({ library(MASS); library(pscl) })
set.seed(20261019)

num <- function(x) sprintf("%.15g", x)
vec <- function(x) paste0("[", paste(num(x), collapse = ", "), "]")
q_disc <- function(p, q) { s <- cumsum(p); which(s >= q * (1 - 1e-12))[1] - 1 }
qs <- c(0.975, 0.9985)

counts_nb <- rnbinom(60, size = 0.8, mu = 3)
counts_zip <- ifelse(runif(80) < 0.4, 0, rpois(80, 4))
counts_zinb <- ifelse(runif(120) < 0.35, 0, rnbinom(120, size = 1.5, mu = 6))
trials <- sample(8:12, 50, replace = TRUE)
pos <- rbinom(50, trials, rbeta(50, 0.6, 9))
cont <- round(rgamma(70, shape = 1.7, scale = 12), 2)

out <- list()
pois <- fitdistr(counts_nb, "poisson")
nb <- fitdistr(counts_nb, "negative binomial")
nb_zinb <- zeroinfl(y ~ 1 | 1, data = data.frame(y = counts_nb), dist = "negbin")
out$nb <- sprintf('{"x": %s, "zinb_aic": %s, "poisson_lambda": %s, "poisson_loglik": %s, "nb_size": %s, "nb_mu": %s, "nb_loglik": %s, "nb_quantiles": %s, "poisson_quantiles": %s}',
  vec(counts_nb), num(AIC(nb_zinb)), num(pois$estimate), num(pois$loglik), num(nb$estimate["size"]), num(nb$estimate["mu"]), num(nb$loglik),
  vec(qnbinom(qs, size = nb$estimate["size"], mu = nb$estimate["mu"])), vec(qpois(qs, pois$estimate)))

zip <- zeroinfl(y ~ 1 | 1, data = data.frame(y = counts_zip), dist = "poisson")
lam <- exp(coef(zip)[1]); pz <- plogis(coef(zip)[2])
pzip <- c(pz + (1 - pz) * dpois(0, lam), (1 - pz) * dpois(1:200, lam))
out$zip <- sprintf('{"x": %s, "lambda": %s, "pi": %s, "loglik": %s, "quantiles": %s}', vec(counts_zip), num(lam), num(pz),
  num(as.numeric(logLik(zip))), vec(sapply(qs, function(q) q_disc(pzip, q))))

zinb <- zeroinfl(y ~ 1 | 1, data = data.frame(y = counts_zinb), dist = "negbin")
mu <- exp(coef(zinb)[1]); pz <- plogis(coef(zinb)[2]); k <- zinb$theta
pzinb <- c(pz + (1 - pz) * dnbinom(0, size = k, mu = mu), (1 - pz) * dnbinom(1:2000, size = k, mu = mu))
out$zinb <- sprintf('{"x": %s, "mu": %s, "size": %s, "pi": %s, "loglik": %s, "quantiles": %s}', vec(counts_zinb), num(mu), num(k),
  num(pz), num(as.numeric(logLik(zinb))), vec(sapply(qs, function(q) q_disc(pzinb, q))))

bb_ll <- function(par, x, n) {
  mu <- plogis(par[1]); s <- exp(par[2]); a <- mu * s; b <- (1 - mu) * s
  sum(lchoose(n, x) + lbeta(x + a, n - x + b) - lbeta(a, b))
}
fit <- optim(c(qlogis(sum(pos) / sum(trials)), 1), function(p) -bb_ll(p, pos, trials), method = "BFGS",
  control = list(reltol = 1e-14, maxit = 1000))
mu <- plogis(fit$par[1]); s <- exp(fit$par[2])
pbb <- function(n) exp(lchoose(n, 0:n) + lbeta(0:n + mu * s, n - 0:n + (1 - mu) * s) - lbeta(mu * s, (1 - mu) * s))
pbin <- sum(pos) / sum(trials)
out$bb <- sprintf('{"x": %s, "trials": %s, "binomial_p": %s, "binomial_loglik": %s, "mu": %s, "size": %s, "loglik": %s, "quantiles_n10": %s, "binomial_quantiles_n10": %s}',
  vec(pos), vec(trials), num(pbin), num(sum(dbinom(pos, trials, pbin, log = TRUE))), num(mu), num(s), num(-fit$value),
  vec(sapply(qs, function(q) q_disc(pbb(10), q))), vec(qbinom(qs, 10, pbin)))

g <- fitdistr(cont, "gamma", start = list(shape = 1.5, rate = 0.05), lower = c(1e-4, 1e-6))
ln <- fitdistr(cont, "lognormal")
w <- fitdistr(cont, "weibull", lower = c(1e-4, 1e-4))
out$cont <- sprintf('{"x": %s, "gamma_loglik": %s, "gamma_quantiles": %s, "lognormal_loglik": %s, "lognormal_quantiles": %s, "weibull_loglik": %s, "weibull_quantiles": %s}',
  vec(cont), num(g$loglik), vec(qgamma(qs, g$estimate["shape"], g$estimate["rate"])), num(ln$loglik),
  vec(qlnorm(qs, ln$estimate["meanlog"], ln$estimate["sdlog"])), num(w$loglik),
  vec(qweibull(qs, w$estimate["shape"], w$estimate["scale"])))

writeLines(paste0("{", paste(sprintf('"%s": %s', names(out), unlist(out)), collapse = ",\n"), "}"), "monitoring_limits_reference.json")

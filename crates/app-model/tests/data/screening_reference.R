# Reference values for tests/screening.rs (R 4.3.3, MASS 7.3).
# Run: Rscript screening_reference.R
suppressMessages(library(MASS))
p <- function(name, v) cat(name, "=", paste(sprintf("%.17g", v), collapse = ", "), "\n")
set.seed(42)
n <- 24
trt <- factor(rep(c("ctl", "trt"), each = 12))
batch <- factor(rep(c("b1", "b2", "b3"), times = 8))
tissue <- rep(c("liver", "lung"), times = 12)
y <- sapply(1:4, function(j) round(10 + (j - 1) * 0.8 * (trt == "trt") + rnorm(n), 2))
k <- sapply(1:4, function(j) rnbinom(n, mu = 5 * exp((j - 2) * 0.4 * (trt == "trt")), size = 3))
for (j in 1:4) p(paste0("y", j), y[, j])
for (j in 1:4) p(paste0("k", j), k[, j])

# Linear model per response: partial F tests (drop1), BH per effect.
lin <- lapply(1:4, function(j) drop1(lm(y[, j] ~ trt + batch), test = "F"))
p("lin_trt_F", sapply(lin, function(d) d$`F value`[2])); p("lin_trt_p", sapply(lin, function(d) d$`Pr(>F)`[2]))
p("lin_batch_p", sapply(lin, function(d) d$`Pr(>F)`[3]))
p("lin_trt_q", p.adjust(sapply(lin, function(d) d$`Pr(>F)`[2]), "BH"))
p("lin_batch_q_by", p.adjust(sapply(lin, function(d) d$`Pr(>F)`[3]), "BY"))
lf <- lapply(1:4, function(j) lm(y[, j] ~ trt + batch))
p("lin_trt_diff", sapply(lf, function(f) coef(f)[2]))
p("lin_trt_lower", sapply(lf, function(f) confint(f)[2, 1]))
p("lin_batch_range", sapply(lf, function(f) { b <- c(0, coef(f)[3:4]); max(b) - min(b) }))

# Poisson: likelihood-ratio tests (drop1 Chisq), rate ratios.
po <- lapply(1:4, function(j) glm(k[, j] ~ trt + batch, family = poisson))
p("poi_trt_lr", sapply(po, function(f) drop1(f, test = "Chisq")$LRT[2]))
p("poi_trt_p", sapply(po, function(f) drop1(f, test = "Chisq")$`Pr(>Chi)`[2]))
p("poi_trt_ratio", sapply(po, function(f) exp(coef(f)[2])))
p("poi_trt_upper", sapply(po, function(f) exp(confint.default(f)[2, 2])))

# Negative binomial: LR test with theta refitted in the reduced model.
nb <- lapply(1:4, function(j) glm.nb(k[, j] ~ trt + batch))
nb0 <- lapply(1:4, function(j) glm.nb(k[, j] ~ batch))
lr <- sapply(1:4, function(j) 2 * (logLik(nb[[j]]) - logLik(nb0[[j]])))
p("nb_trt_lr", lr); p("nb_trt_p", pchisq(lr, 1, lower.tail = FALSE))
p("nb_theta", sapply(nb, function(f) f$theta))

# Kruskal-Wallis by treatment, within each tissue (subgroups).
for (t in c("liver", "lung")) {
  s <- tissue == t
  kw <- lapply(1:4, function(j) kruskal.test(y[s, j], trt[s]))
  p(paste0("kw_", t, "_h"), sapply(kw, function(r) r$statistic))
  p(paste0("kw_", t, "_p"), sapply(kw, function(r) r$p.value))
  p(paste0("kw_", t, "_q"), p.adjust(sapply(kw, function(r) r$p.value), "BH"))
}

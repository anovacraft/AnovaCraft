# Reference values for crates/stat-linear/tests/spline_reference.rs.
# R 4.3.3, package splines. Run: Rscript spline_reference.R
library(splines)
x <- c(1.2, 2.5, 3.1, 4.8, 5.0, 6.7, 7.3, 8.9, 9.4, 10.6, 11.2, 12.8)
at <- c(-1, 1.2, 3.0, 6.0, 12.8, 15)
p <- function(name, m) cat(name, "=", paste(sprintf("%.17g", t(m)), collapse = ", "), "\n")
b3 <- bs(x, df = 6)              # cubic, 3 interior knots at quantiles
p("bs3_knots", attr(b3, "knots"))
p("bs3_at", predict(b3, at))
b2 <- bs(x, knots = c(4, 9), degree = 2)
p("bs2_at", predict(b2, at))
n3 <- ns(x, df = 4)              # 3 interior knots
p("ns3_knots", attr(n3, "knots"))
p("ns3_at", predict(n3, at))
n1 <- ns(x, knots = 7)
p("ns1_at", predict(n1, at))
# Linear model with a natural spline and a factor (drop-one F tests).
y <- c(3.1, 4.9, 5.2, 7.9, 8.4, 9.1, 8.8, 9.9, 11.3, 10.2, 9.8, 12.5)
g <- factor(c("a", "b", "a", "b", "c", "a", "c", "b", "c", "a", "b", "c"))
m <- lm(y ~ ns(x, df = 3) + g)
p("lm_coef", coef(m))
p("lm_fitted", fitted(m))
a <- drop1(m, test = "F")
p("lm_F", a$`F value`[-1]); p("lm_p", a$`Pr(>F)`[-1])
p("lm_pred", predict(m, data.frame(x = c(0, 6, 14), g = c("a", "b", "c"))))
# Poisson GLM with a cubic B-spline.
cnt <- c(2, 3, 2, 5, 6, 4, 7, 9, 8, 12, 11, 15)
gm <- glm(cnt ~ bs(x, df = 4), family = poisson)
p("glm_coef", coef(gm))
p("glm_dev", deviance(gm))

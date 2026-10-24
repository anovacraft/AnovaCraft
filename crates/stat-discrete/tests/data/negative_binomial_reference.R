# Reference values for crates/stat-discrete/tests/negative_binomial_reference.rs.
# R 4.3.3 with MASS 7.3-60. Run: Rscript negative_binomial_reference.R
library(MASS)
p <- function(name, v) cat(name, "=", paste(sprintf("%.17g", v), collapse = ", "), "\n")
x <- c(0.5, 1.1, 1.8, 2.2, 2.9, 3.4, 4.1, 4.5, 5.2, 5.8, 6.3, 7.0, 7.4, 8.1, 8.8, 9.3)
g <- factor(c("a","b","a","b","a","b","a","b","a","b","a","b","a","b","a","b"))
y <- c(1, 0, 3, 2, 1, 6, 4, 2, 9, 5, 3, 14, 6, 11, 20, 8)
m <- glm.nb(y ~ x + g)
s <- summary(m)$coefficients
p("coef", s[, 1]); p("se", s[, 2]); p("pz", s[, 4])
p("theta", m$theta); p("theta_se", m$SE.theta); p("loglik", as.numeric(logLik(m)))
p("aic", AIC(m)); p("deviance", deviance(m))
r <- glm.nb(y ~ x)
a <- anova(r, m)
p("lr", a$`LR stat.`[2]); p("lr_p", a$`Pr(Chi)`[2])

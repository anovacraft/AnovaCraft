# Reference values for crates/app-model/tests/model_comparisons.rs.
# R 4.3.3, emmeans 1.10.0, lmerTest 3.1-3. Run: Rscript model_comparisons_reference.R
suppressMessages({library(emmeans); library(lmerTest); library(splines)})
emm_options(opt.digits = FALSE)
p <- function(name, v) cat(name, "=", paste(sprintf("%.17g", v), collapse = ", "), "\n")
A <- factor(c("a1","a1","a1","a1","a2","a2","a2","a2","a2","a3","a3","a3","a3","a3","a3","a1","a2","a3","a1","a2"))
B <- factor(c("b1","b2","b1","b2","b1","b2","b1","b2","b2","b1","b2","b1","b2","b1","b1","b2","b1","b2","b1","b2"))
cnt <- c(3, 5, 2, 7, 4, 9, 3, 6, 8, 7, 4, 6, 5, 4, 6, 6, 5, 4, 2, 9)
g <- glm(cnt ~ A * B, family = poisson)
jg <- joint_tests(g, by = "B")
p("glm_slice_p", jg$p.value[jg$`model term` == "A"])
pg <- summary(pairs(emmeans(g, ~ A | B), adjust = "tukey"), infer = TRUE)
p("glm_pairs_est", pg$estimate); p("glm_pairs_p", pg$p.value); p("glm_pairs_lower", pg$asymp.LCL)
# Poisson GLM with a cubic B-spline (4 df).
x <- c(1.2, 2.5, 3.1, 4.8, 5.0, 6.7, 7.3, 8.9, 9.4, 10.6, 11.2, 12.8)
k <- c(2, 3, 2, 5, 6, 4, 7, 9, 8, 12, 11, 15)
gm <- glm(k ~ bs(x, df = 4), family = poisson)
p("glm_spline_coef", coef(gm)); p("glm_spline_dev", deviance(gm))
p("glm_spline_lr", drop1(gm, test = "LRT")$LRT[2])
# Mixed model: random intercepts of subject S, Satterthwaite df.
S <- factor(rep(c("s1","s2","s3","s4","s5"), 4))
y <- c(10.1, 12.3, 9.8, 13.9, 11.2, 15.1, 10.9, 13.0, 16.2, 14.8, 12.1, 14.0, 13.5, 13.2, 14.1, 12.8, 11.9, 12.6, 10.4, 15.6)
y2 <- y + c(1.2, -0.8, 0.5, -1.1, 0.3)[as.integer(S)]
p("mixed_y", y2)
mm <- lmer(y2 ~ A * B + (1 | S), REML = TRUE)
p("mixed_var", as.data.frame(VarCorr(mm))$vcov)
emm_options(lmer.df = "satterthwaite")
# Slice F tests with lmerTest's multi-df Satterthwaite denominator
# (Fai & Cornelius 1996; emmeans' joint_tests combines the df differently).
L1 <- rbind(c(0, 1, 0, 0, 0, 0), c(0, 0, 1, 0, 0, 0))
L2 <- rbind(c(0, 1, 0, 0, 1, 0), c(0, 0, 1, 0, 0, 1))
jm <- rbind(contest(mm, L1, joint = TRUE), contest(mm, L2, joint = TRUE))
p("mixed_slice_F", jm$`F value`); p("mixed_slice_df2", jm$DenDF); p("mixed_slice_p", jm$`Pr(>F)`)
pm <- summary(pairs(emmeans(mm, ~ A | B), adjust = "holm"), infer = TRUE)
p("mixed_pairs_est", pm$estimate); p("mixed_pairs_se", pm$SE); p("mixed_pairs_df", pm$df); p("mixed_pairs_p", pm$p.value)

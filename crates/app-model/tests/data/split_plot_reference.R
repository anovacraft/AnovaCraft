# Reference values for the split-plot analysis of the furnace sample
# (crates/app-model/src/sample/split_plot.rs, test furnace_split_plot_matches_lmer).
#
# Data: split_plot_furnace.csv, the "Furnace coatings (simulated) — split-plot
# design" sample (24 runs in 8 furnace heats = whole plots; temperature held
# fixed within each heat). The model is the design's: main effects and
# two-factor interactions of the three factors in actual units, with a random
# intercept for each whole plot, fitted by REML; F tests of each term with
# Satterthwaite denominator degrees of freedom (lmerTest; each term has one
# degree of freedom, so this is the F = t² test of its coefficient).
# As in the application's linear models, the interaction products are formed
# from the factors centred at their means, so the main effects are the slopes
# at the centre of the data (and the temperature effect is tested against
# whole-plot error, about 6 df).
#
# Run with:  Rscript crates/app-model/tests/data/split_plot_reference.R

suppressMessages(library(lmerTest))
here <- dirname(sub("--file=", "", grep("--file=", commandArgs(FALSE), value = TRUE)))
d <- read.csv(file.path(here, "split_plot_furnace.csv"), check.names = FALSE)
names(d) <- c("WP", "Temp", "Coat", "Pos", "Y")
d$WP <- factor(d$WP)
d$TC <- (d$Temp - mean(d$Temp)) * (d$Coat - mean(d$Coat))
d$TP <- (d$Temp - mean(d$Temp)) * (d$Pos - mean(d$Pos))
d$CP <- (d$Coat - mean(d$Coat)) * (d$Pos - mean(d$Pos))

fit <- lmer(Y ~ Temp + Coat + Pos + TC + TP + CP + (1 | WP), data = d, REML = TRUE)
options(digits = 15)
cat("fixed effects (estimate, std error, df):\n")
print(summary(fit)$coefficients[, c("Estimate", "Std. Error", "df")], digits = 15)
cat("variance components:\n")
print(as.data.frame(VarCorr(fit))[, c("grp", "vcov")], digits = 15)
cat("-2 REML log likelihood:", format(-2 * as.numeric(logLik(fit)), digits = 15), "\n")
cat("F tests (Satterthwaite):\n")
print(anova(fit, ddf = "Satterthwaite")[, c("NumDF", "DenDF", "F value", "Pr(>F)")], digits = 15)

# Simple effects for crates/app-model/src/factorial_repeated.rs: the Phase
# test within each treatment's subjects (phase x hour within, gender between),
# R 4.x with car 3.x.
suppressMessages(library(car))
options(digits = 15, contrasts = c("contr.sum", "contr.poly"))
data(OBrienKaiser)
phase <- factor(rep(c("pretest", "posttest", "followup"), c(5, 5, 5)),
                levels = c("pretest", "posttest", "followup"))
hour <- ordered(rep(1:5, 3))
for (tr in c("A", "B", "control")) {
  d <- subset(OBrienKaiser, treatment == tr)
  mod <- lm(cbind(pre.1, pre.2, pre.3, pre.4, pre.5, post.1, post.2, post.3, post.4, post.5,
                  fup.1, fup.2, fup.3, fup.4, fup.5) ~ gender, data = d)
  s <- summary(Anova(mod, idata = data.frame(phase, hour), idesign = ~phase * hour, type = 3),
               multivariate = FALSE)
  cat(tr, "\n"); print(s$univariate.tests["phase", ]); print(s$pval.adjustments["phase", ])
}

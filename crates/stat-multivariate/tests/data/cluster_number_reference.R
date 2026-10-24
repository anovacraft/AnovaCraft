# Reference values for crates/stat-multivariate/src/cluster_number.rs.
# R 4.x, cluster 2.x. Silhouettes are deterministic given the clustering;
# the gap statistic's reference draws are random, so its tests check the
# arithmetic on fixed numbers (below, clusGap's formulas) and the choice on
# simulated data with known structure.
suppressMessages(library(cluster))
options(digits = 17)
x <- rbind(c(1.0, 1.2), c(1.5, 0.8), c(0.7, 1.9), c(1.9, 1.4), c(5.0, 5.5), c(5.6, 4.9),
           c(4.4, 5.2), c(6.1, 6.0), c(9.0, 1.0), c(8.2, 1.6), c(9.5, 0.4), c(3.5, 3.0))
cl <- c(1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 1)
s <- silhouette(cl, dist(x))
print(s[, "sil_width"])
print(mean(s[, "sil_width"]))
# clusGap's gap and SE.sim for given log W and reference log W* (B = 3).
logW <- c(3.0, 2.0, 1.2, 1.1)
ref <- rbind(c(3.1, 2.6, 2.2, 1.9), c(3.3, 2.8, 2.1, 2.0), c(3.2, 2.7, 2.3, 1.8))
E.logW <- colMeans(ref)
SE.sim <- sqrt((1 + 1 / nrow(ref)) * apply(ref, 2, var))
print(cbind(gap = E.logW - logW, SE.sim))
print(maxSE(E.logW - logW, SE.sim, method = "Tibs2001SEmax"))

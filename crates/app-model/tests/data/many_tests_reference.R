# Reference values for app_model::many_tests (R 4.3.3, base stats only).
# Run: Rscript crates/app-model/tests/data/many_tests_reference.R
g <- c(rep("ctl", 5), rep("trt", 6))
d <- list(
  v1 = c(5.1, 4.9, 5.6, 5.8, 6.0, 6.9, 7.2, 6.6, 7.8, 7.0, 6.4),
  v2 = c(2.0, 2.4, 1.9, 2.2, 2.5, 2.3, 2.1, 2.6, 2.0, 2.4, 2.2),
  v3 = c(10.0, 12.5, 11.2, 9.8, 10.4, 12.9, 13.4, 11.8, 12.2, 14.0, 13.1)
)
welch <- sapply(d, function(x) t.test(x[g == "ctl"], x[g == "trt"])$p.value)
cat("welch p:", format(welch, digits = 17), "\n")
cat("welch BH:", format(p.adjust(welch, "BH"), digits = 17), "\n")
rank <- sapply(d[c("v2", "v1", "v3")], function(x) wilcox.test(x[g == "ctl"], x[g == "trt"], exact = TRUE)$p.value)
cat("rank-sum p (v2, v1, v3):", format(rank, digits = 17), "\n")

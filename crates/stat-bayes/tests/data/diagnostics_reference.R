# Reference values for crates/stat-bayes/tests/sampler_reference.rs:
# rank-normalized R-hat, bulk/tail ESS and MCSE of the fixed draws in
# diagnostics_draws.csv (written by reference_cases.py). R 4.3 with the
# posterior package 1.5 (Debian r-cran-posterior), which implements
# Vehtari et al. (2021).
suppressMessages(library(posterior))
options(digits = 15)
d <- read.csv("diagnostics_draws.csv")
for (p in c("a", "b", "c")) {
  m <- matrix(d[[p]], ncol = 4)  # draws x chains (file is ordered chain, then draw)
  cat(p, rhat(m), ess_bulk(m), ess_tail(m), mcse_mean(m), "\n")
}

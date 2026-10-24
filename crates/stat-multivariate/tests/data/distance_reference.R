# R half of the distance / principal coordinates reference values
# (tests/distance_reference.rs). Run through distance_reference.py, which
# writes the data sets and merges the results; by hand:
#   Rscript distance_reference.R <folder with mixed.csv and counts.csv>
# Packages: cluster (daisy), ape (pcoa); base R cmdscale and hclust.
# Tested with R 4.3, cluster 2.1, ape 5.7.
suppressMessages({ library(cluster); library(ape) })
dir <- commandArgs(trailingOnly = TRUE)[1]
mixed <- read.csv(file.path(dir, "mixed.csv"))
counts <- as.matrix(read.csv(file.path(dir, "counts.csv")))
out <- function(name, x) write.table(x, file.path(dir, paste0(name, ".csv")),
                                     sep = ",", row.names = FALSE, col.names = FALSE)
num <- function(x) formatC(x, digits = 17, format = "g")

# Gower: two numeric columns, an ordered factor, a nominal factor and an
# asymmetric binary variable, with missing values.
df <- data.frame(x1 = mixed$x1, x2 = mixed$x2,
                 ord = factor(mixed$ord, levels = 1:4, ordered = TRUE),
                 nom = factor(mixed$nom),
                 ab = mixed$ab)
g <- as.matrix(daisy(df, metric = "gower", type = list(asymm = "ab")))
out("gower", num(g))

# Bray–Curtis distances are taken from SciPy (python half); recompute here
# only to feed pcoa with identical numbers.
bc <- as.matrix(read.csv(file.path(dir, "braycurtis.csv"), header = FALSE))

# ape::pcoa reports its correction constant only inside a note (15
# digits), so it is recomputed here the way ape computes it and checked
# against that note.
centre <- function(M) { n <- nrow(M); J <- diag(n) - 1 / n; J %*% M %*% J }
constant <- function(D, corr) {
  delta1 <- centre(-0.5 * D^2)
  if (corr == "lingoes") return(-min(eigen(delta1, symmetric = TRUE)$values))
  n <- nrow(D)
  sp <- rbind(cbind(matrix(0, n, n), 2 * delta1), cbind(-diag(n), -4 * centre(-0.5 * D)))
  max(Re(eigen(sp, symmetric = FALSE, only.values = TRUE)$values))
}

pc <- function(tag, D) {
  n <- nrow(D)
  e <- cmdscale(as.dist(D), k = 1, eig = TRUE)$eig
  out(paste0(tag, "_cmdscale_eig"), num(e))
  p <- pcoa(D)
  out(paste0(tag, "_none_vectors"), num(p$vectors))
  for (corr in c("lingoes", "cailliez")) {
    p <- pcoa(D, correction = corr)
    c0 <- constant(D, corr)
    noted <- as.numeric(tail(regmatches(p$note, gregexpr("[0-9]+\\.[0-9]+(e-?[0-9]+)?", p$note))[[1]], 1))
    stopifnot(abs(c0 - noted) < 1e-12 * c0)  # the note keeps 15 digits
    Dc <- if (corr == "lingoes") sqrt(D^2 + 2 * c0) else D + c0
    diag(Dc) <- 0
    out(paste0(tag, "_", corr, "_constant"), num(c0))
    out(paste0(tag, "_", corr, "_eig"), num(cmdscale(as.dist(Dc), k = 1, eig = TRUE)$eig))
    out(paste0(tag, "_", corr, "_corr_eig"), num(p$values$Corr_eig))
    out(paste0(tag, "_", corr, "_relative"), num(p$values$Rel_corr_eig))
    out(paste0(tag, "_", corr, "_broken_stick"), num(p$values$Broken_stick))
    out(paste0(tag, "_", corr, "_vectors"), num(p$vectors.cor))
  }
}
pc("gower", g)
pc("braycurtis", bc)

for (m in c("single", "complete", "average", "ward.D2")) {
  h <- hclust(as.dist(g), method = m)
  out(paste0("hclust_", m, "_height"), num(h$height))
  out(paste0("hclust_", m, "_cut3"), cutree(h, k = 3))
}

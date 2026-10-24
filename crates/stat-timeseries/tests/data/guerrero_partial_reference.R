# Guerrero's lambda for series whose length is not a multiple of the period:
# forecast::BoxCox.lambda drops the incomplete subseries at the start.
# R 4.3.3, forecast 8.21.1. Values are pasted into
# guerrero_lambda_drops_the_leading_partial_cycle (decompose_reference.rs).
suppressMessages(library(forecast))
options(digits = 17)
a <- as.numeric(AirPassengers)
u <- as.numeric(UKgas)
cat("AirPassengers[4:144]", BoxCox.lambda(ts(a[4:144], frequency = 12), method = "guerrero"), "\n")
cat("AirPassengers[1:137]", BoxCox.lambda(ts(a[1:137], frequency = 12), method = "guerrero"), "\n")
cat("UKgas[3:108]", BoxCox.lambda(ts(u[3:108], frequency = 4), method = "guerrero"), "\n")

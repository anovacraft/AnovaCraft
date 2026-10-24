"""Write treatment_study.csv: an INVENTED observational study of a cardiac
rehabilitation programme (400 patients), the sample for treatment-effect
analysis. Enrolment depends on age, sex, severity, smoking and region, and so
do the outcomes, so unadjusted comparisons are confounded. The programme's
true effect on fitness gain is larger for patients under 65.

    python3 treatment_study.py   # NumPy; seed 2519
"""
import csv
import pathlib

import numpy as np

rng = np.random.default_rng(2519)
n = 400
age = np.clip(np.round(rng.normal(64, 9, n)), 35, 90)
female = rng.random(n) < 0.42
severity = np.round(np.clip(rng.normal(5, 2, n), 0.5, 10), 1)
smoker = rng.random(n) < 0.3
region = rng.choice(["North", "South", "East"], n, p=[0.4, 0.35, 0.25])
lin = -0.4 - 0.06 * (age - 64) + 0.5 * female - 0.25 * (severity - 5) - 0.6 * smoker + 0.4 * (region == "South")
p = 1 / (1 + np.exp(-lin))
enrolled = rng.random(n) < p
young = age < 65
effect = np.where(young, 4.0, 1.5)
fitness = 10 - 0.15 * (age - 64) - 0.8 * (severity - 5) - 1.5 * smoker + 1.0 * female + effect * enrolled + rng.normal(0, 3, n)
lin_r = -1.2 + 0.03 * (age - 64) + 0.3 * (severity - 5) + 0.5 * smoker - 0.7 * enrolled
readmitted = rng.random(n) < 1 / (1 + np.exp(-lin_r))
visits = rng.poisson(np.exp(1.0 + 0.02 * (age - 64) + 0.1 * (severity - 5) + 0.6 * enrolled))
out = pathlib.Path(__file__).with_name("treatment_study.csv")
with out.open("w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["Patient", "Programme", "Age", "Sex", "Severity", "Smoker", "Region", "Age group",
                "Fitness gain", "Readmitted", "Clinic visits"])
    for i in range(n):
        w.writerow([i + 1, "Enrolled" if enrolled[i] else "Usual care", int(age[i]), "F" if female[i] else "M",
                    severity[i], "Yes" if smoker[i] else "No", region[i], "Under 65" if young[i] else "65 and over",
                    round(float(fitness[i]), 1), int(readmitted[i]), int(visits[i])])
print(out, enrolled.sum(), "enrolled")

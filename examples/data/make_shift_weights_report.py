"""Write shift_weights_report.xlsx: two sheets laid out like a printed report
(a title, a plant line, a blank row, the table from C4 and a footnote), for
trying the Open Data dialog. Simulated values. Run: python3 make_shift_weights_report.py"""
import random

from openpyxl import Workbook

random.seed(20261019)
book = Workbook()
for i, (line, mean) in enumerate([("Line 1", 500.0), ("Line 2", 502.0)]):
    sheet = book.active if i == 0 else book.create_sheet()
    sheet.title = line
    sheet["A1"] = f"Fill weights by shift, {line}"
    sheet["A2"] = "Plant"
    sheet["B2"] = "North"
    for col, name in zip("CDE", ["Shift", "Operator", "Weight"]):
        sheet[f"{col}4"] = name
    rows = 12
    for r in range(rows):
        sheet[f"C{5 + r}"] = "ABC"[r % 3]
        sheet[f"D{5 + r}"] = ["Ana", "Ben", "Chen", "Dee"][r % 4]
        sheet[f"E{5 + r}"] = round(random.gauss(mean, 1.5), 1)
    sheet[f"A{6 + rows}"] = "Weights in grams; checked hourly."
book.save("shift_weights_report.xlsx")

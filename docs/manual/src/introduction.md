# Introduction

AnovaCraft is an open-source desktop application for statistical analysis,
design of experiments and predictive modelling. You open a data table, choose
an analysis from the menus, and get a report of collapsible sections with
tables and interactive plots. Selecting rows in any plot or in the data grid
highlights the same rows everywhere, so you can follow an observation from a
residual plot back to the data and into every other report.

This is the **Reference Manual**: it explains everything the application
does in detail, for people who want every option and the method behind each
result. If you are new to the app or to statistics, start with the
**[Quick Guide](../../guide/src/SUMMARY.md)** (Help ▸ Quick guide), which
describes every menu item in plain language and links back here for the
details. Both manuals are built into the app and open as PDFs from the Help
menu.

This manual is organised the way you work:

- **Getting started** installs and runs the app, walks around the window and
  runs a first analysis on a sample dataset.
- **Working with data** covers opening and saving files, editing the grid,
  formula columns, table operations such as sorting, joining and reshaping,
  data from databases and the web, missing values and multiple imputation,
  and row states (selecting, excluding and hiding rows).
- **Reports and plots** explains the parts every analysis shares: report
  sections and their options menus, plot navigation, saving plots and
  results, the Response Explorer, the graph canvas, figure pages, 3-D
  plots and dashboards.
- **Analyses** has a chapter per menu (Describe, Compare, Model,
  Multivariate, Design, Quality, Reliability and Series and Signals), with
  separate chapters for the larger topics such as Bayesian analysis, each
  kind of predictive model, nonlinear regression, curve templates and
  text analysis.
- **Automation** is the reference for Rhai scripts, the Script window and
  recording your actions as a script.

Every method in AnovaCraft follows a published statistical definition, and
each analysis is tested against an independent reference (for example the
NIST Statistical Reference Datasets). Chapters name the method and its
reference so you can read more about it.

AnovaCraft is an independent implementation built from published
statistical literature; it contains no code, scripts or designs from any
commercial statistics package and is not affiliated with their makers. It
uses general statistical names for its methods (for example, adaptive
regression splines and random forests).

> **Tip:** File ▸ Open sample data lists datasets chosen to show each kind of
> analysis. Each chapter names the sample that goes with it.

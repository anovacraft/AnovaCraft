//! Assay curve templates: ready-made nonlinear regressions for common
//! laboratory experiments.
//!
//! A template is a model formula ([`crate::nonlinear_analysis`]) written for
//! one kind of experiment, with its parameters named in that field's terms,
//! starting values computed from the data, default bounds, weighting and
//! sharing across data sets, and the quantities the field reads off the fit
//! (an inhibition constant, a half-life, a quantitation range…) with their
//! intervals ([`stat_linear::nonlinear::assay`]). Choosing a template fills
//! in an ordinary [`NonlinearRequest`]; every option of nonlinear regression
//! (global fits, weights, robust fitting, profile intervals, inverse
//! prediction) still applies, and the report adds the template's own
//! sections.
//!
//! Templates are named by the standard model names of the cited literature:
//!
//! * **Standard curves** — four- and five-parameter logistic calibration
//!   curves (Rodbard, "Statistical quality control and routine data
//!   processing for radioimmunoassays", *Clin. Chem.* 20, 1974; Gottschalk &
//!   Dunn, *Anal. Biochem.* 343, 2005), acceptance by ICH M10 (2022).
//! * **Receptor binding** — saturation, competition and kinetic binding
//!   (Hulme & Trevethick, "Ligand binding assays at equilibrium", *Br. J.
//!   Pharmacol.* 161, 2010; Cheng & Prusoff, *Biochem. Pharmacol.* 22, 1973;
//!   Motulsky & Mahan, *Mol. Pharmacol.* 25, 1984).
//! * **Enzyme kinetics** — Michaelis–Menten, substrate inhibition, Hill,
//!   reversible inhibition mechanisms, tight-binding inhibition (Morrison,
//!   *Biochim. Biophys. Acta* 185, 1969) and the integrated rate equation
//!   (Schnell & Mendoza, *J. Theor. Biol.* 187, 1997; Goudar, Sonnad &
//!   Duggleby, *Biochim. Biophys. Acta* 1429, 1999); Cornish-Bowden,
//!   *Fundamentals of Enzyme Kinetics*, 4th ed., 2012.
//! * **Pharmacological dose–response** — logistic agonist and inhibitor
//!   curves, bell-shaped and biphasic curves, Gaddum–Schild antagonism
//!   (Arunlakshana & Schild, *Br. J. Pharmacol.* 14, 1959; Lew & Angus,
//!   *Trends Pharmacol. Sci.* 16, 1995), the operational model of agonism
//!   (Black & Leff, *Proc. R. Soc. B* 220, 1983) and relative potency from
//!   parallel curves (Finney, *Statistical Method in Biological Assay*, 1978).
//! * **Growth, decay and survival** — modified Gompertz growth (Zwietering
//!   et al., *Appl. Environ. Microbiol.* 56, 1990), Baranyi–Roberts growth
//!   (*Int. J. Food Microbiol.* 23, 1994), first-order decay and association,
//!   linear-quadratic cell survival (Fowler, *Br. J. Radiol.* 62, 1989;
//!   Franken et al., *Nat. Protoc.* 1, 2006), the Boltzmann activation curve
//!   and the Arrhenius equation.

use crate::formula::column_reference;
use crate::nonlinear_analysis::{NonlinearRequest, ParameterSetting, Weighting};
use crate::project::invalid;
use stat_core::Result;
use stat_data::DataTable;
use stat_linear::nonlinear::assay::{self, AcceptanceLimits};

/// Gas constant, J·mol⁻¹·K⁻¹ (CODATA 2018, exact).
pub const GAS_CONSTANT: f64 = 8.314_462_618;

/// Kind of experiment a template is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateGroup {
    /// Calibration (standard) curves of immunoassays and other
    /// ligand-binding assays.
    StandardCurves,
    /// Receptor–ligand binding at equilibrium and over time.
    Binding,
    /// Enzyme kinetics and inhibition.
    Enzyme,
    /// Pharmacological concentration–response.
    DoseResponse,
    /// Growth, decay, survival and other kinetic curves.
    GrowthDecay,
}

impl TemplateGroup {
    /// Every group, in menu order.
    pub const ALL: [Self; 5] =
        [Self::StandardCurves, Self::Binding, Self::Enzyme, Self::DoseResponse, Self::GrowthDecay];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::StandardCurves => "Immunoassay standard curves",
            Self::Binding => "Receptor binding",
            Self::Enzyme => "Enzyme kinetics",
            Self::DoseResponse => "Pharmacological dose–response",
            Self::GrowthDecay => "Growth, decay and survival",
        }
    }
}

/// The templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum CurveTemplate {
    FourPl,
    FivePl,
    SaturationOneSite,
    SaturationTotalNonspecific,
    SaturationTwoSites,
    CompetitionOneSite,
    CompetitionTwoSites,
    HomologousCompetition,
    Association,
    AssociationSeveral,
    Dissociation,
    CompetitorKinetics,
    MichaelisMenten,
    SubstrateInhibition,
    Allosteric,
    Competitive,
    Uncompetitive,
    Noncompetitive,
    Mixed,
    TightBinding,
    ProgressCurve,
    Agonist,
    Inhibitor,
    BellShaped,
    Biphasic,
    GaddumSchild,
    Operational,
    RelativePotency,
    Gompertz,
    Baranyi,
    OnePhaseDecay,
    TwoPhaseDecay,
    OnePhaseAssociation,
    PlateauDecay,
    LinearQuadratic,
    Boltzmann,
    Arrhenius,
}

/// What the second column of a template holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondKind {
    /// A number in the model (an inhibitor or antagonist concentration).
    Numeric,
    /// Which of two kinds of measurement a row is: total binding (1, or a
    /// label containing "total") or nonspecific binding (0 or anything else).
    Indicator,
}

/// The second column a template reads besides the x column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecondRole {
    /// Dialog label.
    pub label: &'static str,
    /// What it holds.
    pub kind: SecondKind,
}

/// How a template uses a data-set column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSets {
    /// Optional, as in any nonlinear regression (one curve per data set).
    Optional,
    /// Required: the template compares data sets (the label names them).
    Required(&'static str),
}

/// A number the template needs that is not in the data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantSpec {
    /// Key in the project file.
    pub key: &'static str,
    /// Dialog label.
    pub label: &'static str,
    /// Default (NaN: none).
    pub default: f64,
    /// Whether the fit needs it (otherwise only some derived quantities do).
    pub required: bool,
}

/// One parameter of a template.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// Name (an identifier, used in the formula and the report).
    pub name: &'static str,
    /// What it means.
    pub meaning: &'static str,
    /// Default lower bound (−∞: none).
    pub lower: f64,
    /// Default upper bound (+∞: none).
    pub upper: f64,
    /// Shared by every data set by default.
    pub shared: bool,
}

/// A quantity computed from a template's parameters.
#[derive(Debug, Clone, Copy)]
pub struct DerivedSpec {
    /// Name in the report.
    pub label: &'static str,
    /// The value from the parameters (template order) and constants.
    pub value: fn(&[f64], &[f64]) -> f64,
    /// `Some(k)` when the value depends on parameter `k` alone, monotonically:
    /// its interval is that parameter's interval mapped through `value`.
    pub single: Option<usize>,
    /// Constants (indices) it needs; it is left out when one is missing.
    pub needs: &'static [usize],
}

/// Extra report sections of a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extra {
    /// None beyond the derived quantities.
    None,
    /// Back-calculated standards, acceptance, precision profile and
    /// sample concentrations.
    StandardCurve,
    /// The four reversible inhibition mechanisms compared.
    Mechanism,
    /// Dose ratios and the Schild plot.
    Schild,
    /// Relative potency and the parallelism test.
    Potency,
}

const INF: f64 = f64::INFINITY;
const NINF: f64 = f64::NEG_INFINITY;

const fn par(name: &'static str, meaning: &'static str) -> ParamSpec {
    ParamSpec { name, meaning, lower: NINF, upper: INF, shared: true }
}

const fn pos(name: &'static str, meaning: &'static str) -> ParamSpec {
    ParamSpec { name, meaning, lower: 0.0, upper: INF, shared: true }
}

const fn frac(name: &'static str, meaning: &'static str) -> ParamSpec {
    ParamSpec { name, meaning, lower: 0.0, upper: 1.0, shared: true }
}

const fn own(mut p: ParamSpec) -> ParamSpec {
    p.shared = false;
    p
}

const fn constant(key: &'static str, label: &'static str, default: f64, required: bool) -> ConstantSpec {
    ConstantSpec { key, label, default, required }
}

const NAN: f64 = f64::NAN;
const LN2: f64 = std::f64::consts::LN_2;

fn exp10(v: f64) -> f64 {
    10f64.powf(v)
}

impl CurveTemplate {
    /// Every template, in menu order.
    pub const ALL: [Self; 37] = [
        Self::FourPl,
        Self::FivePl,
        Self::SaturationOneSite,
        Self::SaturationTotalNonspecific,
        Self::SaturationTwoSites,
        Self::CompetitionOneSite,
        Self::CompetitionTwoSites,
        Self::HomologousCompetition,
        Self::Association,
        Self::AssociationSeveral,
        Self::Dissociation,
        Self::CompetitorKinetics,
        Self::MichaelisMenten,
        Self::SubstrateInhibition,
        Self::Allosteric,
        Self::Competitive,
        Self::Uncompetitive,
        Self::Noncompetitive,
        Self::Mixed,
        Self::TightBinding,
        Self::ProgressCurve,
        Self::Agonist,
        Self::Inhibitor,
        Self::BellShaped,
        Self::Biphasic,
        Self::GaddumSchild,
        Self::Operational,
        Self::RelativePotency,
        Self::Gompertz,
        Self::Baranyi,
        Self::OnePhaseDecay,
        Self::TwoPhaseDecay,
        Self::OnePhaseAssociation,
        Self::PlateauDecay,
        Self::LinearQuadratic,
        Self::Boltzmann,
        Self::Arrhenius,
    ];

    /// The templates of `group`, in menu order.
    pub fn of_group(group: TemplateGroup) -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(move |t| t.group() == group)
    }

    /// The group it belongs to.
    #[must_use]
    pub fn group(self) -> TemplateGroup {
        use CurveTemplate as T;
        match self {
            T::FourPl | T::FivePl => TemplateGroup::StandardCurves,
            T::SaturationOneSite
            | T::SaturationTotalNonspecific
            | T::SaturationTwoSites
            | T::CompetitionOneSite
            | T::CompetitionTwoSites
            | T::HomologousCompetition
            | T::Association
            | T::AssociationSeveral
            | T::Dissociation
            | T::CompetitorKinetics => TemplateGroup::Binding,
            T::MichaelisMenten
            | T::SubstrateInhibition
            | T::Allosteric
            | T::Competitive
            | T::Uncompetitive
            | T::Noncompetitive
            | T::Mixed
            | T::TightBinding
            | T::ProgressCurve => TemplateGroup::Enzyme,
            T::Agonist
            | T::Inhibitor
            | T::BellShaped
            | T::Biphasic
            | T::GaddumSchild
            | T::Operational
            | T::RelativePotency => TemplateGroup::DoseResponse,
            _ => TemplateGroup::GrowthDecay,
        }
    }

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        use CurveTemplate as T;
        match self {
            T::FourPl => "standard_4pl",
            T::FivePl => "standard_5pl",
            T::SaturationOneSite => "saturation_one_site",
            T::SaturationTotalNonspecific => "saturation_total_nonspecific",
            T::SaturationTwoSites => "saturation_two_sites",
            T::CompetitionOneSite => "competition_one_site",
            T::CompetitionTwoSites => "competition_two_sites",
            T::HomologousCompetition => "homologous_competition",
            T::Association => "association",
            T::AssociationSeveral => "association_several",
            T::Dissociation => "dissociation",
            T::CompetitorKinetics => "competitor_kinetics",
            T::MichaelisMenten => "michaelis_menten",
            T::SubstrateInhibition => "substrate_inhibition",
            T::Allosteric => "allosteric_hill",
            T::Competitive => "competitive_inhibition",
            T::Uncompetitive => "uncompetitive_inhibition",
            T::Noncompetitive => "noncompetitive_inhibition",
            T::Mixed => "mixed_inhibition",
            T::TightBinding => "tight_binding",
            T::ProgressCurve => "progress_curve",
            T::Agonist => "agonist",
            T::Inhibitor => "inhibitor",
            T::BellShaped => "bell_shaped",
            T::Biphasic => "biphasic",
            T::GaddumSchild => "gaddum_schild",
            T::Operational => "operational_model",
            T::RelativePotency => "relative_potency",
            T::Gompertz => "modified_gompertz",
            T::Baranyi => "baranyi_roberts",
            T::OnePhaseDecay => "one_phase_decay",
            T::TwoPhaseDecay => "two_phase_decay",
            T::OnePhaseAssociation => "one_phase_association",
            T::PlateauDecay => "plateau_then_decay",
            T::LinearQuadratic => "linear_quadratic_survival",
            T::Boltzmann => "boltzmann",
            T::Arrhenius => "arrhenius",
        }
    }

    /// The template with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        use CurveTemplate as T;
        match self {
            T::FourPl => "Four-parameter logistic (4PL) standard curve",
            T::FivePl => "Five-parameter logistic (5PL) standard curve",
            T::SaturationOneSite => "Saturation binding, one site (specific binding)",
            T::SaturationTotalNonspecific => "Saturation binding, total and nonspecific together",
            T::SaturationTwoSites => "Saturation binding, two sites",
            T::CompetitionOneSite => "Competition binding, one site (Ki)",
            T::CompetitionTwoSites => "Competition binding, two sites (Ki)",
            T::HomologousCompetition => "Homologous competition (Kd and Bmax)",
            T::Association => "Association kinetics, one ligand concentration",
            T::AssociationSeveral => "Association kinetics, several ligand concentrations",
            T::Dissociation => "Dissociation kinetics",
            T::CompetitorKinetics => "Kinetics of an unlabelled competitor",
            T::MichaelisMenten => "Michaelis–Menten (Km, Vmax, kcat)",
            T::SubstrateInhibition => "Substrate inhibition",
            T::Allosteric => "Allosteric (Hill) kinetics",
            T::Competitive => "Competitive inhibition",
            T::Uncompetitive => "Uncompetitive inhibition",
            T::Noncompetitive => "Noncompetitive inhibition",
            T::Mixed => "Mixed inhibition",
            T::TightBinding => "Tight-binding inhibition (Morrison)",
            T::ProgressCurve => "Progress curve (integrated Michaelis–Menten)",
            T::Agonist => "Agonist concentration–response (EC50, ECF)",
            T::Inhibitor => "Inhibitor concentration–response (IC50)",
            T::BellShaped => "Bell-shaped concentration–response",
            T::Biphasic => "Biphasic concentration–response",
            T::GaddumSchild => "Antagonist shift: Gaddum–Schild (pA2)",
            T::Operational => "Operational model of agonism (τ, KA)",
            T::RelativePotency => "Relative potency from parallel curves",
            T::Gompertz => "Microbial growth: modified Gompertz",
            T::Baranyi => "Microbial growth: Baranyi–Roberts",
            T::OnePhaseDecay => "One-phase exponential decay",
            T::TwoPhaseDecay => "Two-phase exponential decay",
            T::OnePhaseAssociation => "One-phase exponential association",
            T::PlateauDecay => "Plateau followed by one-phase decay",
            T::LinearQuadratic => "Cell survival: linear-quadratic",
            T::Boltzmann => "Boltzmann activation curve (V50)",
            T::Arrhenius => "Arrhenius equation (activation energy)",
        }
    }

    /// The model, in words and symbols, for the dialog and the report.
    #[must_use]
    pub fn equation(self) -> &'static str {
        use CurveTemplate as T;
        match self {
            T::FourPl => "Y = D + (A − D) / (1 + (X/C)^B)",
            T::FivePl => "Y = D + (A − D) / (1 + (X/C)^B)^G",
            T::SaturationOneSite => "Y = Bmax·X / (Kd + X)",
            T::SaturationTotalNonspecific => {
                "Total: Y = Bmax·X/(Kd + X) + NS·X + Background;  nonspecific: Y = NS·X + Background"
            }
            T::SaturationTwoSites => "Y = Bmax1·X/(Kd1 + X) + Bmax2·X/(Kd2 + X)",
            T::CompetitionOneSite => "Y = Bottom + (Top − Bottom) / (1 + 10^(X − logIC50));  Ki = IC50 / (1 + [L]/Kd)",
            T::CompetitionTwoSites => {
                "Y = Bottom + (Top − Bottom)·(F/(1 + 10^(X − logIC50_1)) + (1 − F)/(1 + 10^(X − logIC50_2)))"
            }
            T::HomologousCompetition => "Y = Bmax·[L] / ([L] + 10^X + 10^logKd) + NS",
            T::Association => "Y = Ymax·(1 − exp(−kob·X))",
            T::AssociationSeveral => {
                "Y = Bmax·[L]/([L] + koff/kon)·(1 − exp(−(kon·[L] + koff)·X)),  [L] from the second column"
            }
            T::Dissociation => "Y = (Y0 − NS)·exp(−koff·X) + NS",
            T::CompetitorKinetics => {
                "Motulsky–Mahan: labelled ligand (k1, k2 known) with an unlabelled competitor (k3, k4)"
            }
            T::MichaelisMenten => "Y = Vmax·X / (Km + X);  kcat = Vmax / [E]",
            T::SubstrateInhibition => "Y = Vmax·X / (Km + X·(1 + X/Ksi))",
            T::Allosteric => "Y = Vmax·X^h / (Khalf^h + X^h)",
            T::Competitive => "Y = Vmax·X / (Km·(1 + I/Ki) + X)",
            T::Uncompetitive => "Y = Vmax·X / (Km + X·(1 + I/Ki))",
            T::Noncompetitive => "Y = Vmax/(1 + I/Ki) · X/(Km + X)",
            T::Mixed => "Y = Vmax·X / (Km·(1 + I/Ki) + X·(1 + I/(alpha·Ki)))",
            T::TightBinding => "Y = V0·(1 − ((E + X + Kiapp) − √((E + X + Kiapp)² − 4·E·X)) / (2·E))",
            T::ProgressCurve => "P = S0 − Km·W((S0/Km)·exp((S0 − Vmax·t)/Km)),  W the Lambert W function",
            T::Agonist => "Y = Bottom + (Top − Bottom) / (1 + 10^((logEC50 − X)·Hill))",
            T::Inhibitor => "Y = Bottom + (Top − Bottom) / (1 + 10^((X − logIC50)·Hill))",
            T::BellShaped => {
                "Y = Plateau + (Bottom1 − Plateau)/(1 + 10^((X − logEC50_1)·nH1)) + (Bottom2 − Plateau)/(1 + 10^((logEC50_2 − X)·nH2))"
            }
            T::Biphasic => {
                "Y = Bottom + (Top − Bottom)·(F/(1 + 10^((logEC50_1 − X)·nH1)) + (1 − F)/(1 + 10^((logEC50_2 − X)·nH2)))"
            }
            T::GaddumSchild => {
                "Y = Bottom + (Top − Bottom) / (1 + 10^((logEC50 + log(1 + ([B]·10^pA2)^SchildSlope) − X)·Hill))"
            }
            T::Operational => "Y = Basal + (Em − Basal)·τⁿ[A]ⁿ / (τⁿ[A]ⁿ + ([A] + KA)ⁿ),  [A] = 10^X",
            T::RelativePotency => {
                "Y = Bottom + (Top − Bottom) / (1 + 10^((logEC50 − X)·Hill)), logEC50 per preparation"
            }
            T::Gompertz => "Y = A·exp(−exp(mu·e/A·(lambda − X) + 1)),  Y = ln(N/N0)",
            T::Baranyi => "Y = y0 + mu·At − ln(1 + (exp(mu·At) − 1)/exp(ymax − y0)),  lag lambda",
            T::OnePhaseDecay => "Y = (Y0 − Plateau)·exp(−K·X) + Plateau",
            T::TwoPhaseDecay => "Y = Plateau + SpanFast·exp(−Kfast·X) + SpanSlow·exp(−Kslow·X)",
            T::OnePhaseAssociation => "Y = Y0 + (Plateau − Y0)·(1 − exp(−K·X))",
            T::PlateauDecay => "Y = Y0 for X < X0;  Plateau + (Y0 − Plateau)·exp(−K·(X − X0)) after",
            T::LinearQuadratic => "Y = PE·exp(−(alpha·D + beta·D²)),  Y the fraction of plated cells forming colonies",
            T::Boltzmann => "Y = Bottom + (Top − Bottom) / (1 + exp((V50 − X)/Slope))",
            T::Arrhenius => "k = exp(lnA − 1000·Ea / (R·T)),  Ea in kJ/mol, T in kelvin",
        }
    }

    /// One or two sentences on the experiment and the data layout.
    #[must_use]
    pub fn description(self) -> &'static str {
        use CurveTemplate as T;
        match self {
            T::FourPl => {
                "Calibration standards of known concentration (X, not logged) and their signal. Rows with a signal but no concentration are samples read off the curve. Weighted 1/Y² by default; standards are back-calculated and checked against ICH M10 limits."
            }
            T::FivePl => {
                "As the 4PL curve, with an asymmetry parameter G for curves that bend more sharply at one end. G = 1 is the 4PL curve."
            }
            T::SaturationOneSite => {
                "Specific binding (total minus nonspecific) against free radioligand concentration. Kd is in the units of X; Bmax in the units of Y."
            }
            T::SaturationTotalNonspecific => {
                "Total and nonspecific binding in one table, with a column saying which each row is (1 or a label containing \"total\" for total binding). Fits both curves at once, sharing NS and Background."
            }
            T::SaturationTwoSites => {
                "Specific binding to two independent classes of sites with different affinities. Compare with the one-site template by AICc."
            }
            T::CompetitionOneSite => {
                "Bound radioligand against log₁₀ of the unlabelled competitor's molar concentration, slope fixed at −1. Give the radioligand's concentration and Kd for Ki."
            }
            T::CompetitionTwoSites => {
                "As one-site competition, for a competitor that sees two classes of sites; F is the fraction of high-affinity sites."
            }
            T::HomologousCompetition => {
                "The unlabelled form of the radioligand competes with it (X = log₁₀ molar concentration of unlabelled ligand). Assumes no ligand depletion and that labelling does not change affinity."
            }
            T::Association => {
                "Specific binding over time at one radioligand concentration. With that concentration and koff from a dissociation experiment, gives kon and Kd."
            }
            T::AssociationSeveral => {
                "Specific binding over time at several radioligand concentrations (second column, molar). Fits kon and koff together; Kd = koff/kon."
            }
            T::Dissociation => {
                "Binding over time after dissociation is started (excess unlabelled ligand or dilution). NS is the plateau."
            }
            T::CompetitorKinetics => {
                "Association of a radioligand of known kon (k1) and koff (k2) over time, with the unlabelled competitor's concentration in the second column (0 for the control curve). Gives the competitor's kon (k3) and koff (k4)."
            }
            T::MichaelisMenten => {
                "Initial rate against substrate concentration. Give the enzyme concentration for kcat (in the rate's units of concentration per time over enzyme concentration)."
            }
            T::SubstrateInhibition => {
                "Rates that fall again at high substrate; Ksi is the dissociation constant for the second, inhibitory substrate molecule."
            }
            T::Allosteric => {
                "Sigmoid rate curve of a cooperative enzyme; h is the Hill coefficient and Khalf the substrate concentration at half Vmax."
            }
            T::Competitive | T::Uncompetitive | T::Noncompetitive | T::Mixed => {
                "Rates at several substrate concentrations (X) and inhibitor concentrations (second column, 0 for no inhibitor), fitted together. The report compares all four mechanisms by AICc."
            }
            T::TightBinding => {
                "Rate against inhibitor concentration when the inhibitor binds so tightly that free and total concentrations differ. Give the enzyme concentration (same units as X); with the substrate concentration and Km, a competitive inhibitor's Ki is also given."
            }
            T::ProgressCurve => {
                "Product formed over time in one reaction, from initial substrate S0 (same units as the product). Fits Km and Vmax from the whole curve."
            }
            T::Agonist => {
                "Response against log₁₀ of the agonist's molar concentration. ECF is the concentration for F% of the response range (F = 80 by default)."
            }
            T::Inhibitor => {
                "Response against log₁₀ of the inhibitor's molar concentration, falling from Top to Bottom."
            }
            T::BellShaped => {
                "A response that rises to a plateau and falls again (two sites or mechanisms acting in opposite directions)."
            }
            T::Biphasic => {
                "A response that rises in two steps; F is the fraction of the range in the first, more potent phase."
            }
            T::GaddumSchild => {
                "Agonist curves (X = log₁₀ molar agonist) at several antagonist concentrations (second column, molar; 0 for the control). Fits pA2 and the Schild slope globally; with the slope fixed at 1 (equal bounds), pA2 is the pKB."
            }
            T::Operational => {
                "Curves of several agonists in one tissue or cell system (data-set column), X = log₁₀ molar agonist. Em, Basal and the slope n are shared; each agonist gets its own affinity (logKA) and efficacy (logTau)."
            }
            T::RelativePotency => {
                "Concentration–response curves of a standard and one or more test preparations (data-set column) with common Bottom, Top and Hill. Tests parallelism and gives each test preparation's potency relative to the standard."
            }
            T::Gompertz => {
                "Log counts of a culture over time as ln(N/N0). A is the asymptote, mu the maximum specific growth rate and lambda the lag time."
            }
            T::Baranyi => {
                "Log counts ln N over time: initial level y0, maximum ymax, maximum specific growth rate mu and lag time lambda."
            }
            T::OnePhaseDecay => "A quantity falling exponentially to a plateau; the half-life is ln 2/K.",
            T::TwoPhaseDecay => {
                "Decay with a fast and a slow component, each with its own rate constant and half-life."
            }
            T::OnePhaseAssociation => "A quantity rising exponentially to a plateau; the half-time is ln 2/K.",
            T::PlateauDecay => "A level that holds until X0 and then decays exponentially.",
            T::LinearQuadratic => {
                "Clonogenic assay: X = dose (Gy), Y = colonies counted ÷ cells plated. PE is the plating efficiency; the surviving fraction is exp(−αD − βD²). Weighted 1/Y by default."
            }
            T::Boltzmann => {
                "Activation of an ion channel (or any two-state process) against membrane potential: V50 is the half-activation voltage and Slope the slope factor."
            }
            T::Arrhenius => {
                "Rate constant against absolute temperature (kelvin). Ea is the activation energy in kJ/mol; lnA the log of the pre-exponential factor. Weighted 1/Y² by default."
            }
        }
    }

    /// Label of the x column.
    #[must_use]
    pub fn x_label(self) -> &'static str {
        use CurveTemplate as T;
        match self {
            T::FourPl | T::FivePl => "Concentration of the standards",
            T::SaturationOneSite | T::SaturationTotalNonspecific | T::SaturationTwoSites => "Radioligand concentration",
            T::CompetitionOneSite | T::CompetitionTwoSites => "log₁₀ competitor concentration (M)",
            T::HomologousCompetition => "log₁₀ unlabelled ligand concentration (M)",
            T::Association
            | T::AssociationSeveral
            | T::Dissociation
            | T::CompetitorKinetics
            | T::ProgressCurve
            | T::Gompertz
            | T::Baranyi
            | T::OnePhaseDecay
            | T::TwoPhaseDecay
            | T::OnePhaseAssociation
            | T::PlateauDecay => "Time",
            T::MichaelisMenten
            | T::SubstrateInhibition
            | T::Allosteric
            | T::Competitive
            | T::Uncompetitive
            | T::Noncompetitive
            | T::Mixed => "Substrate concentration",
            T::TightBinding => "Inhibitor concentration",
            T::Agonist | T::BellShaped | T::Biphasic | T::GaddumSchild | T::Operational | T::RelativePotency => {
                "log₁₀ agonist concentration (M)"
            }
            T::Inhibitor => "log₁₀ inhibitor concentration (M)",
            T::LinearQuadratic => "Dose (Gy)",
            T::Boltzmann => "Membrane potential (mV)",
            T::Arrhenius => "Temperature (K)",
        }
    }

    /// Label of the response.
    #[must_use]
    pub fn y_label(self) -> &'static str {
        use CurveTemplate as T;
        match self {
            T::FourPl | T::FivePl => "Signal",
            T::MichaelisMenten
            | T::SubstrateInhibition
            | T::Allosteric
            | T::Competitive
            | T::Uncompetitive
            | T::Noncompetitive
            | T::Mixed
            | T::TightBinding => "Rate",
            T::ProgressCurve => "Product",
            T::Gompertz => "ln(N/N0)",
            T::Baranyi => "ln N",
            T::LinearQuadratic => "Colonies per cell plated",
            T::Arrhenius => "Rate constant",
            T::Agonist
            | T::Inhibitor
            | T::BellShaped
            | T::Biphasic
            | T::GaddumSchild
            | T::Operational
            | T::RelativePotency => "Response",
            T::Boltzmann => "Activation",
            _ => "Binding",
        }
    }

    /// The second column, if the template reads one.
    #[must_use]
    pub fn second(self) -> Option<SecondRole> {
        use CurveTemplate as T;
        let numeric = |label| Some(SecondRole { label, kind: SecondKind::Numeric });
        match self {
            T::SaturationTotalNonspecific => {
                Some(SecondRole { label: "Total or nonspecific", kind: SecondKind::Indicator })
            }
            T::AssociationSeveral => numeric("Radioligand concentration (M)"),
            T::CompetitorKinetics => numeric("Competitor concentration (M)"),
            T::Competitive | T::Uncompetitive | T::Noncompetitive | T::Mixed => numeric("Inhibitor concentration"),
            T::GaddumSchild => numeric("Antagonist concentration (M)"),
            _ => None,
        }
    }

    /// How the template uses a data-set column.
    #[must_use]
    pub fn data_sets(self) -> DataSets {
        match self {
            Self::Operational => DataSets::Required("Agonist"),
            Self::RelativePotency => DataSets::Required("Preparation"),
            _ => DataSets::Optional,
        }
    }

    /// Numbers the template needs that are not in the data.
    #[must_use]
    pub fn constants(self) -> &'static [ConstantSpec] {
        use CurveTemplate as T;
        match self {
            T::CompetitionOneSite | T::CompetitionTwoSites => {
                const {
                    &[
                        constant("ligand", "Radioligand concentration [L] (M)", NAN, false),
                        constant("ligand_kd", "Radioligand Kd (M)", NAN, false),
                    ]
                }
            }
            T::HomologousCompetition => const { &[constant("ligand", "Radioligand concentration [L] (M)", NAN, true)] },
            T::Association => {
                const {
                    &[
                        constant("ligand", "Radioligand concentration [L]", NAN, false),
                        constant("koff", "koff from a dissociation experiment", NAN, false),
                    ]
                }
            }
            T::CompetitorKinetics => {
                const {
                    &[
                        constant("ligand", "Radioligand concentration [L] (M)", NAN, true),
                        constant("k1", "Radioligand kon, k1 (per M per time)", NAN, true),
                        constant("k2", "Radioligand koff, k2 (per time)", NAN, true),
                    ]
                }
            }
            T::MichaelisMenten | T::ProgressCurve => {
                const { &[constant("enzyme", "Enzyme concentration [E] (for kcat)", NAN, false)] }
            }
            T::TightBinding => {
                const {
                    &[
                        constant("enzyme", "Enzyme concentration [E] (same units as X)", NAN, true),
                        constant("substrate", "Substrate concentration (for Ki)", NAN, false),
                        constant("km", "Km of the substrate (for Ki)", NAN, false),
                    ]
                }
            }
            T::Agonist | T::Inhibitor => const { &[constant("f", "F for ECF (%)", 80.0, false)] },
            T::Arrhenius => const { &[constant("t_ref", "Reference temperature (K)", 298.15, false)] },
            _ => &[],
        }
    }

    /// The constant needed by the formula itself: the progress curve's
    /// initial substrate is one too.
    fn formula_constants(self) -> &'static [ConstantSpec] {
        match self {
            Self::ProgressCurve => const { &[constant("s0", "Initial substrate S0", NAN, true)] },
            _ => &[],
        }
    }

    /// Every constant: the formula's own first, then [`Self::constants`].
    #[must_use]
    pub fn all_constants(self) -> Vec<ConstantSpec> {
        self.formula_constants().iter().chain(self.constants()).copied().collect()
    }

    /// The parameters, in model order.
    #[must_use]
    pub fn parameters(self) -> &'static [ParamSpec] {
        use CurveTemplate as T;
        match self {
            T::FourPl => {
                const {
                    &[
                        par("A", "Response at zero concentration"),
                        par("B", "Slope factor"),
                        pos("C", "Inflection point (concentration)"),
                        par("D", "Response at infinite concentration"),
                    ]
                }
            }
            T::FivePl => {
                const {
                    &[
                        par("A", "Response at zero concentration"),
                        par("B", "Slope factor"),
                        pos("C", "Inflection point of the 4PL part (concentration)"),
                        par("D", "Response at infinite concentration"),
                        pos("G", "Asymmetry (1: symmetric 4PL)"),
                    ]
                }
            }
            T::SaturationOneSite => {
                const { &[pos("Bmax", "Maximal specific binding"), pos("Kd", "Equilibrium dissociation constant")] }
            }
            T::SaturationTotalNonspecific => {
                const {
                    &[
                        pos("Bmax", "Maximal specific binding"),
                        pos("Kd", "Equilibrium dissociation constant"),
                        par("NS", "Slope of nonspecific binding"),
                        par("Background", "Binding with no radioligand"),
                    ]
                }
            }
            T::SaturationTwoSites => {
                const {
                    &[
                        pos("Bmax1", "Sites of the first class"),
                        pos("Kd1", "Kd of the first class"),
                        pos("Bmax2", "Sites of the second class"),
                        pos("Kd2", "Kd of the second class"),
                    ]
                }
            }
            T::CompetitionOneSite => {
                const {
                    &[
                        par("Top", "Binding with no competitor"),
                        par("Bottom", "Binding at saturating competitor (nonspecific)"),
                        par("logIC50", "log₁₀ of the concentration halving specific binding"),
                    ]
                }
            }
            T::CompetitionTwoSites => {
                const {
                    &[
                        par("Top", "Binding with no competitor"),
                        par("Bottom", "Binding at saturating competitor"),
                        frac("F", "Fraction of high-affinity sites"),
                        par("logIC50_1", "log₁₀ IC50 at the high-affinity sites"),
                        par("logIC50_2", "log₁₀ IC50 at the low-affinity sites"),
                    ]
                }
            }
            T::HomologousCompetition => {
                const { &[pos("Bmax", "Total sites"), par("logKd", "log₁₀ Kd (M)"), par("NS", "Nonspecific binding")] }
            }
            T::Association => {
                const { &[pos("Ymax", "Binding at equilibrium"), pos("kob", "Observed association rate constant")] }
            }
            T::AssociationSeveral => {
                const {
                    &[
                        pos("Bmax", "Total sites"),
                        pos("kon", "Association rate constant (per M per time)"),
                        pos("koff", "Dissociation rate constant (per time)"),
                    ]
                }
            }
            T::Dissociation => {
                const {
                    &[
                        par("Y0", "Binding at time zero"),
                        par("NS", "Binding remaining (nonspecific)"),
                        pos("koff", "Dissociation rate constant"),
                    ]
                }
            }
            T::CompetitorKinetics => {
                const {
                    &[
                        pos("Bmax", "Total sites"),
                        pos("k3", "Competitor association rate constant (per M per time)"),
                        pos("k4", "Competitor dissociation rate constant (per time)"),
                    ]
                }
            }
            T::MichaelisMenten => const { &[pos("Vmax", "Maximal rate"), pos("Km", "Michaelis constant")] },
            T::SubstrateInhibition => {
                const {
                    &[
                        pos("Vmax", "Maximal rate without inhibition"),
                        pos("Km", "Michaelis constant"),
                        pos("Ksi", "Substrate inhibition constant"),
                    ]
                }
            }
            T::Allosteric => {
                const {
                    &[
                        pos("Vmax", "Maximal rate"),
                        pos("Khalf", "Substrate concentration at half Vmax"),
                        pos("h", "Hill coefficient"),
                    ]
                }
            }
            T::Competitive | T::Uncompetitive | T::Noncompetitive => {
                const {
                    &[
                        pos("Vmax", "Maximal rate without inhibitor"),
                        pos("Km", "Michaelis constant without inhibitor"),
                        pos("Ki", "Inhibition constant"),
                    ]
                }
            }
            T::Mixed => {
                const {
                    &[
                        pos("Vmax", "Maximal rate without inhibitor"),
                        pos("Km", "Michaelis constant without inhibitor"),
                        pos("Ki", "Dissociation constant from the free enzyme"),
                        pos(
                            "alpha",
                            "Ratio of the constants (large: competitive, 1: noncompetitive, small: uncompetitive)",
                        ),
                    ]
                }
            }
            T::TightBinding => {
                const { &[pos("V0", "Rate without inhibitor"), pos("Kiapp", "Apparent inhibition constant")] }
            }
            T::ProgressCurve => const { &[pos("Vmax", "Maximal rate"), pos("Km", "Michaelis constant")] },
            T::Agonist => {
                const {
                    &[
                        par("Bottom", "Response with no agonist"),
                        par("Top", "Maximal response"),
                        par("logEC50", "log₁₀ of the concentration for half the response range"),
                        par("Hill", "Hill slope"),
                    ]
                }
            }
            T::Inhibitor => {
                const {
                    &[
                        par("Top", "Response with no inhibitor"),
                        par("Bottom", "Response at full inhibition"),
                        par("logIC50", "log₁₀ of the concentration for half the response range"),
                        par("Hill", "Hill slope"),
                    ]
                }
            }
            T::BellShaped => {
                const {
                    &[
                        par("Bottom1", "Response at low concentrations"),
                        par("Plateau", "Response between the two phases"),
                        par("Bottom2", "Response at high concentrations"),
                        par("logEC50_1", "log₁₀ EC50 of the rising phase"),
                        par("logEC50_2", "log₁₀ EC50 of the falling phase"),
                        par("nH1", "Slope of the rising phase"),
                        par("nH2", "Slope of the falling phase"),
                    ]
                }
            }
            T::Biphasic => {
                const {
                    &[
                        par("Bottom", "Response with no agonist"),
                        par("Top", "Maximal response"),
                        frac("F", "Fraction of the range in the first phase"),
                        par("logEC50_1", "log₁₀ EC50 of the first phase"),
                        par("logEC50_2", "log₁₀ EC50 of the second phase"),
                        par("nH1", "Slope of the first phase"),
                        par("nH2", "Slope of the second phase"),
                    ]
                }
            }
            T::GaddumSchild => {
                const {
                    &[
                        par("Bottom", "Response with no agonist"),
                        par("Top", "Maximal response"),
                        par("logEC50", "log₁₀ EC50 without antagonist"),
                        par("Hill", "Hill slope"),
                        par("pA2", "−log₁₀ of the antagonist concentration doubling the EC50"),
                        par("SchildSlope", "Schild slope (1 for simple competitive antagonism)"),
                    ]
                }
            }
            T::Operational => {
                const {
                    &[
                        par("Basal", "Response with no agonist"),
                        par("Em", "Maximal response of the system"),
                        pos("n", "Slope of the transducer function"),
                        own(par("logKA", "log₁₀ of the agonist's dissociation constant (M)")),
                        own(par("logTau", "log₁₀ of the agonist's efficacy τ")),
                    ]
                }
            }
            T::RelativePotency => {
                const {
                    &[
                        par("Bottom", "Response with no agonist (shared)"),
                        par("Top", "Maximal response (shared)"),
                        own(par("logEC50", "log₁₀ EC50 of each preparation")),
                        par("Hill", "Hill slope (shared)"),
                    ]
                }
            }
            T::Gompertz => {
                const {
                    &[
                        par("A", "Asymptote ln(Nmax/N0)"),
                        pos("mu", "Maximum specific growth rate"),
                        par("lambda", "Lag time"),
                    ]
                }
            }
            T::Baranyi => {
                const {
                    &[
                        par("y0", "Initial ln N"),
                        par("ymax", "Maximal ln N"),
                        pos("mu", "Maximum specific growth rate"),
                        pos("lambda", "Lag time"),
                    ]
                }
            }
            T::OnePhaseDecay => {
                const { &[par("Y0", "Value at time zero"), par("Plateau", "Value approached"), pos("K", "Rate constant")] }
            }
            T::TwoPhaseDecay => {
                const {
                    &[
                        par("Plateau", "Value approached"),
                        par("SpanFast", "Size of the fast component"),
                        pos("Kfast", "Rate constant of the fast component"),
                        par("SpanSlow", "Size of the slow component"),
                        pos("Kslow", "Rate constant of the slow component"),
                    ]
                }
            }
            T::OnePhaseAssociation => {
                const { &[par("Y0", "Value at time zero"), par("Plateau", "Value approached"), pos("K", "Rate constant")] }
            }
            T::PlateauDecay => {
                const {
                    &[
                        par("Y0", "Level before the decay"),
                        par("Plateau", "Value approached"),
                        par("X0", "Time the decay starts"),
                        pos("K", "Rate constant"),
                    ]
                }
            }
            T::LinearQuadratic => {
                const {
                    &[
                        pos("PE", "Plating efficiency (colonies per cell plated at zero dose)"),
                        pos("alpha", "Linear coefficient (per Gy)"),
                        pos("beta", "Quadratic coefficient (per Gy²)"),
                    ]
                }
            }
            T::Boltzmann => {
                const {
                    &[
                        par("Bottom", "Lower plateau"),
                        par("Top", "Upper plateau"),
                        par("V50", "Half-activation potential"),
                        par("Slope", "Slope factor (mV per e-fold change)"),
                    ]
                }
            }
            T::Arrhenius => {
                const { &[par("lnA", "ln of the pre-exponential factor"), par("Ea", "Activation energy (kJ/mol)")] }
            }
        }
    }

    /// Default observation weights.
    #[must_use]
    pub fn default_weighting(self) -> Weighting {
        match self {
            Self::FourPl | Self::FivePl | Self::Arrhenius => Weighting::InverseResponseSquared,
            Self::LinearQuadratic => Weighting::InverseResponse,
            _ => Weighting::Equal,
        }
    }

    /// Extra report sections.
    #[must_use]
    pub fn extra(self) -> Extra {
        match self {
            Self::FourPl | Self::FivePl => Extra::StandardCurve,
            Self::Competitive | Self::Uncompetitive | Self::Noncompetitive | Self::Mixed => Extra::Mechanism,
            Self::GaddumSchild => Extra::Schild,
            Self::RelativePotency => Extra::Potency,
            _ => Extra::None,
        }
    }

    /// Derived quantities. Constant indices refer to [`Self::all_constants`].
    #[must_use]
    pub fn derived(self) -> &'static [DerivedSpec] {
        use CurveTemplate as T;
        const fn d(
            label: &'static str,
            value: fn(&[f64], &[f64]) -> f64,
            single: Option<usize>,
            needs: &'static [usize],
        ) -> DerivedSpec {
            DerivedSpec { label, value, single, needs }
        }
        match self {
            T::CompetitionOneSite => {
                const {
                    &[
                        d("IC50 (M)", |p, _| exp10(p[2]), Some(2), &[]),
                        d("Ki (M)", |p, c| assay::cheng_prusoff_ki(exp10(p[2]), c[0], c[1]), Some(2), &[0, 1]),
                        d("pKi", |p, c| -assay::cheng_prusoff_ki(exp10(p[2]), c[0], c[1]).log10(), Some(2), &[0, 1]),
                    ]
                }
            }
            T::CompetitionTwoSites => {
                const {
                    &[
                        d("IC50, high affinity (M)", |p, _| exp10(p[3]), Some(3), &[]),
                        d("IC50, low affinity (M)", |p, _| exp10(p[4]), Some(4), &[]),
                        d(
                            "Ki, high affinity (M)",
                            |p, c| assay::cheng_prusoff_ki(exp10(p[3]), c[0], c[1]),
                            Some(3),
                            &[0, 1],
                        ),
                        d(
                            "Ki, low affinity (M)",
                            |p, c| assay::cheng_prusoff_ki(exp10(p[4]), c[0], c[1]),
                            Some(4),
                            &[0, 1],
                        ),
                    ]
                }
            }
            T::HomologousCompetition => {
                const { &[d("Kd (M)", |p, _| exp10(p[1]), Some(1), &[]), d("pKd", |p, _| -p[1], Some(1), &[])] }
            }
            T::Association => {
                const {
                    &[
                        d("Half-time ln 2/kob", |p, _| assay::half_life(p[1]), Some(1), &[]),
                        d("kon = (kob − koff)/[L]", |p, c| (p[1] - c[1]) / c[0], Some(1), &[0, 1]),
                        d("Kd = koff/kon", |p, c| assay::kinetic_kd((p[1] - c[1]) / c[0], c[1]), Some(1), &[0, 1]),
                    ]
                }
            }
            T::AssociationSeveral => {
                const {
                    &[
                        d("Kd = koff/kon (M)", |p, _| assay::kinetic_kd(p[1], p[2]), None, &[]),
                        d("Dissociation half-life ln 2/koff", |p, _| assay::half_life(p[2]), Some(2), &[]),
                    ]
                }
            }
            T::Dissociation => const { &[d("Half-life ln 2/koff", |p, _| assay::half_life(p[2]), Some(2), &[])] },
            T::CompetitorKinetics => {
                const {
                    &[
                        d("Competitor Kd = k4/k3 (M)", |p, _| assay::kinetic_kd(p[1], p[2]), None, &[]),
                        d("Residence time 1/k4", |p, _| 1.0 / p[2], Some(2), &[]),
                        d("Dissociation half-life ln 2/k4", |p, _| assay::half_life(p[2]), Some(2), &[]),
                    ]
                }
            }
            T::MichaelisMenten => {
                const {
                    &[
                        d("kcat = Vmax/[E]", |p, c| p[0] / c[0], Some(0), &[0]),
                        d("kcat/Km", |p, c| p[0] / c[0] / p[1], None, &[0]),
                    ]
                }
            }
            T::ProgressCurve => {
                const {
                    &[
                        d("kcat = Vmax/[E]", |p, c| p[0] / c[1], Some(0), &[1]),
                        d("kcat/Km", |p, c| p[0] / c[1] / p[1], None, &[1]),
                    ]
                }
            }
            T::Mixed => const { &[d("alpha·Ki (from the enzyme–substrate complex)", |p, _| p[2] * p[3], None, &[])] },
            T::TightBinding => {
                const { &[d("Ki = Kiapp/(1 + [S]/Km), competitive", |p, c| p[1] / (1.0 + c[1] / c[2]), Some(1), &[1, 2])] }
            }
            T::Agonist => {
                const {
                    &[
                        d("EC50 (M)", |p, _| exp10(p[2]), Some(2), &[]),
                        d("pEC50", |p, _| -p[2], Some(2), &[]),
                        d("ECF (M), F from the settings", |p, c| assay::ec_f(exp10(p[2]), p[3], c[0]), None, &[0]),
                    ]
                }
            }
            T::Inhibitor => {
                const {
                    &[
                        d("IC50 (M)", |p, _| exp10(p[2]), Some(2), &[]),
                        d("pIC50", |p, _| -p[2], Some(2), &[]),
                        // For a falling curve, ICF is F% of the way down from Top.
                        d("ICF (M), F from the settings", |p, c| assay::ec_f(exp10(p[2]), p[3], c[0]), None, &[0]),
                    ]
                }
            }
            T::BellShaped | T::Biphasic => {
                const {
                    &[
                        d("EC50, first phase (M)", |p, _| exp10(p[3]), Some(3), &[]),
                        d("EC50, second phase (M)", |p, _| exp10(p[4]), Some(4), &[]),
                    ]
                }
            }
            T::GaddumSchild => {
                const {
                    &[
                        d("KB = 10^−pA2 (M), valid when the Schild slope is 1", |p, _| exp10(-p[4]), Some(4), &[]),
                        d("EC50 without antagonist (M)", |p, _| exp10(p[2]), Some(2), &[]),
                    ]
                }
            }
            T::Operational => {
                const {
                    &[
                        d("KA (M)", |p, _| exp10(p[3]), Some(3), &[]),
                        d("τ (efficacy)", |p, _| exp10(p[4]), Some(4), &[]),
                        d("log(τ/KA) (transduction coefficient)", |p, _| p[4] - p[3], None, &[]),
                    ]
                }
            }
            T::RelativePotency => const { &[d("EC50 (M)", |p, _| exp10(p[2]), Some(2), &[])] },
            T::Gompertz => const { &[d("Doubling time ln 2/mu (Y in natural logs)", |p, _| LN2 / p[1], Some(1), &[])] },
            T::Baranyi => const { &[d("Doubling time ln 2/mu (Y in natural logs)", |p, _| LN2 / p[2], Some(2), &[])] },
            T::OnePhaseDecay | T::OnePhaseAssociation => {
                const { &[d("Half-life ln 2/K", |p, _| assay::half_life(p[2]), Some(2), &[])] }
            }
            T::PlateauDecay => const { &[d("Half-life ln 2/K", |p, _| assay::half_life(p[3]), Some(3), &[])] },
            T::TwoPhaseDecay => {
                const {
                    &[
                        d("Half-life, fast", |p, _| assay::half_life(p[2]), Some(2), &[]),
                        d("Half-life, slow", |p, _| assay::half_life(p[4]), Some(4), &[]),
                        d("Percent fast (%)", |p, _| 100.0 * p[1] / (p[1] + p[3]), None, &[]),
                    ]
                }
            }
            T::LinearQuadratic => {
                const {
                    &[
                        d("alpha/beta (Gy)", |p, _| p[1] / p[2], None, &[]),
                        d("SF2, surviving fraction at 2 Gy", |p, _| (-2.0 * p[1] - 4.0 * p[2]).exp(), None, &[]),
                        d("D10, dose for 10% survival (Gy)", |p, _| dose_for_survival(p[1], p[2], 0.1), None, &[]),
                    ]
                }
            }
            T::Arrhenius => {
                const {
                    &[
                        d("A, pre-exponential factor", |p, _| p[0].exp(), Some(0), &[]),
                        d(
                            "k at the reference temperature",
                            |p, c| (p[0] - 1000.0 * p[1] / (GAS_CONSTANT * c[0])).exp(),
                            None,
                            &[0],
                        ),
                    ]
                }
            }
            _ => &[],
        }
    }

    /// The model formula. `x` and `z` are the x column's and the second
    /// column's references (for an indicator, an expression that is 1 for
    /// total binding), `c` the values of [`Self::all_constants`].
    #[must_use]
    pub fn formula(self, x: &str, z: &str, c: &[f64]) -> String {
        use CurveTemplate as T;
        let k = |i: usize| lit(c.get(i).copied().unwrap_or(f64::NAN));
        match self {
            T::FourPl => format!("D + (A - D) / (1.0 + ({x} / C) ** B)"),
            T::FivePl => format!("D + (A - D) / (1.0 + ({x} / C) ** B) ** G"),
            T::SaturationOneSite => format!("Bmax * {x} / (Kd + {x})"),
            T::SaturationTotalNonspecific => {
                format!("NS * {x} + Background + {z} * Bmax * {x} / (Kd + {x})")
            }
            T::SaturationTwoSites => format!("Bmax1 * {x} / (Kd1 + {x}) + Bmax2 * {x} / (Kd2 + {x})"),
            T::CompetitionOneSite => format!("Bottom + (Top - Bottom) / (1.0 + 10.0 ** ({x} - logIC50))"),
            T::CompetitionTwoSites => format!(
                "Bottom + (Top - Bottom) * (F / (1.0 + 10.0 ** ({x} - logIC50_1)) + (1.0 - F) / (1.0 + 10.0 ** ({x} - logIC50_2)))"
            ),
            T::HomologousCompetition => {
                let l = k(0);
                format!("Bmax * {l} / ({l} + 10.0 ** {x} + 10.0 ** logKd) + NS")
            }
            T::Association => format!("Ymax * (1.0 - exp(-kob * {x}))"),
            T::AssociationSeveral => {
                format!("Bmax * {z} / ({z} + koff / kon) * (1.0 - exp(-(kon * {z} + koff) * {x}))")
            }
            T::Dissociation => format!("(Y0 - NS) * exp(-koff * {x}) + NS"),
            T::CompetitorKinetics => format!(
                "let mm_l = {}; let mm_k1 = {}; let mm_k2 = {}; let mm_i = {z};\n\
                 let mm_ka = mm_k1 * mm_l + mm_k2; let mm_kb = k3 * mm_i + k4;\n\
                 let mm_s = sqrt((mm_ka - mm_kb) ** 2 + 4.0 * mm_k1 * k3 * mm_l * mm_i);\n\
                 let mm_kf = 0.5 * (mm_ka + mm_kb + mm_s); let mm_ks = 0.5 * (mm_ka + mm_kb - mm_s);\n\
                 let mm_q = Bmax * mm_k1 * mm_l / (mm_kf - mm_ks);\n\
                 mm_q * (k4 * (mm_kf - mm_ks) / (mm_kf * mm_ks) + (k4 - mm_kf) / mm_kf * exp(-mm_kf * {x}) - (k4 - mm_ks) / mm_ks * exp(-mm_ks * {x}))",
                k(0),
                k(1),
                k(2)
            ),
            T::MichaelisMenten => format!("Vmax * {x} / (Km + {x})"),
            T::SubstrateInhibition => format!("Vmax * {x} / (Km + {x} * (1.0 + {x} / Ksi))"),
            T::Allosteric => format!("Vmax * {x} ** h / (Khalf ** h + {x} ** h)"),
            T::Competitive => format!("Vmax * {x} / (Km * (1.0 + {z} / Ki) + {x})"),
            T::Uncompetitive => format!("Vmax * {x} / (Km + {x} * (1.0 + {z} / Ki))"),
            T::Noncompetitive => format!("Vmax / (1.0 + {z} / Ki) * {x} / (Km + {x})"),
            T::Mixed => format!("Vmax * {x} / (Km * (1.0 + {z} / Ki) + {x} * (1.0 + {z} / (alpha * Ki)))"),
            T::TightBinding => {
                let e = k(0);
                format!(
                    "let tb_s = {e} + {x} + Kiapp;\nV0 * (1.0 - (tb_s - sqrt(tb_s ** 2 - 4.0 * {e} * {x})) / (2.0 * {e}))"
                )
            }
            T::ProgressCurve => {
                let s0 = k(0);
                format!("{s0} - Km * wright_omega(ln({s0} / Km) + ({s0} - Vmax * {x}) / Km)")
            }
            T::Agonist | T::RelativePotency => {
                format!("Bottom + (Top - Bottom) / (1.0 + 10.0 ** ((logEC50 - {x}) * Hill))")
            }
            T::Inhibitor => format!("Bottom + (Top - Bottom) / (1.0 + 10.0 ** (({x} - logIC50) * Hill))"),
            T::BellShaped => format!(
                "Plateau + (Bottom1 - Plateau) / (1.0 + 10.0 ** (({x} - logEC50_1) * nH1)) + (Bottom2 - Plateau) / (1.0 + 10.0 ** ((logEC50_2 - {x}) * nH2))"
            ),
            T::Biphasic => format!(
                "Bottom + (Top - Bottom) * (F / (1.0 + 10.0 ** ((logEC50_1 - {x}) * nH1)) + (1.0 - F) / (1.0 + 10.0 ** ((logEC50_2 - {x}) * nH2)))"
            ),
            T::GaddumSchild => format!(
                "Bottom + (Top - Bottom) / (1.0 + 10.0 ** ((logEC50 + log10(1.0 + ({z} * 10.0 ** pA2) ** SchildSlope) - {x}) * Hill))"
            ),
            T::Operational => format!(
                "let op_a = 10.0 ** {x}; let op_ta = (10.0 ** logTau * op_a) ** n;\n\
                 Basal + (Em - Basal) * op_ta / (op_ta + (op_a + 10.0 ** logKA) ** n)"
            ),
            T::Gompertz => format!("A * exp(-exp(mu * {} / A * (lambda - {x}) + 1.0))", lit(std::f64::consts::E)),
            T::Baranyi => format!(
                "let br_h = mu * lambda;\n\
                 let br_a = {x} + ln(exp(-mu * {x}) + exp(-br_h) - exp(-mu * {x} - br_h)) / mu;\n\
                 y0 + mu * br_a - ln(1.0 + (exp(mu * br_a) - 1.0) / exp(ymax - y0))"
            ),
            T::OnePhaseDecay => format!("(Y0 - Plateau) * exp(-K * {x}) + Plateau"),
            T::TwoPhaseDecay => format!("Plateau + SpanFast * exp(-Kfast * {x}) + SpanSlow * exp(-Kslow * {x})"),
            T::OnePhaseAssociation => format!("Y0 + (Plateau - Y0) * (1.0 - exp(-K * {x}))"),
            T::PlateauDecay => {
                format!("if {x} < X0 {{ Y0 }} else {{ Plateau + (Y0 - Plateau) * exp(-K * ({x} - X0)) }}")
            }
            T::LinearQuadratic => format!("PE * exp(-(alpha * {x} + beta * {x} ** 2))"),
            T::Boltzmann => format!("Bottom + (Top - Bottom) / (1.0 + exp((V50 - {x}) / Slope))"),
            T::Arrhenius => format!("exp(lnA - 1000.0 * Ea / ({} * {x}))", lit(GAS_CONSTANT)),
        }
    }

    /// Starting values from the data (`z` NaN when the template reads no
    /// second column; 1/0 for an indicator), in model order.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn starts(self, d: &StartData) -> Vec<f64> {
        use CurveTemplate as T;
        let p = Profile::new(&d.x, &d.y);
        let (y0, y1) = (p.first_y(), p.last_y());
        let (ymin, ymax) = p.y_range();
        let mid = p.x_at_fraction(0.5);
        let half_level = |prof: &Profile, frac: f64| prof.x_at_level(prof.y_range().1 * frac);
        let rate_from_half = |prof: &Profile| {
            let t = prof.x_at_fraction(0.5) - prof.first_x();
            if t > 0.0 { LN2 / t } else { 1.0 / (prof.last_x() - prof.first_x()).max(1e-12) }
        };
        let c = |i: usize| d.constants.get(i).copied().unwrap_or(f64::NAN);
        match self {
            T::FourPl | T::FivePl => {
                let xs: Vec<f64> = p.points.iter().map(|q| q.0).filter(|v| *v > 0.0).collect();
                let geo =
                    if xs.is_empty() { 1.0 } else { (xs.iter().map(|v| v.ln()).sum::<f64>() / xs.len() as f64).exp() };
                let c50 = p.x_at_fraction_log(0.5).filter(|v| *v > 0.0).unwrap_or(geo);
                let mut s = vec![y0, 1.0, c50, y1];
                if self == T::FivePl {
                    s.push(1.0);
                }
                s
            }
            T::SaturationOneSite => vec![1.2 * ymax, positive_or(half_level(&p, 0.5), p.median_x())],
            T::SaturationTotalNonspecific => {
                let (ns_x, ns_y) = d.select(|z| z == 0.0);
                let ns = through_origin(&ns_x, &ns_y).max(0.0);
                let (tx, ty) = d.select(|z| z != 0.0);
                let spec: Vec<f64> = tx.iter().zip(&ty).map(|(x, y)| y - ns * x).collect();
                let sp = Profile::new(&tx, &spec);
                let bmax = 1.2 * sp.y_range().1;
                vec![bmax, positive_or(sp.x_at_level(sp.y_range().1 * 0.5), sp.median_x()), ns, 0.0]
            }
            T::SaturationTwoSites => {
                let kd = positive_or(half_level(&p, 0.5), p.median_x());
                vec![0.6 * ymax, kd / 5.0, 0.6 * ymax, kd * 5.0]
            }
            T::CompetitionOneSite => vec![y0, y1, mid],
            T::CompetitionTwoSites => vec![y0, y1, 0.5, mid - 1.0, mid + 1.0],
            T::HomologousCompetition => {
                let l = c(0);
                let ns = 0.9 * ymin;
                let ic50 = exp10(mid);
                let kd = if ic50 - l > 0.0 { ic50 - l } else { ic50 / 2.0 };
                let bmax = (y0 - ns) * (l + kd) / l;
                vec![if bmax.is_finite() && bmax > 0.0 { bmax } else { y0 }, kd.log10(), ns]
            }
            T::Association => vec![1.05 * ymax, positive_or(LN2 / half_level(&p, 0.5), 1.0)],
            T::AssociationSeveral => {
                // kob of each ligand concentration from its half-time, then
                // kob = kon·[L] + koff by least squares.
                let mut levels: Vec<f64> = d.z.iter().copied().filter(|v| v.is_finite()).collect();
                levels.sort_by(f64::total_cmp);
                levels.dedup();
                let (mut ls, mut kobs, mut plateaus) = (Vec::new(), Vec::new(), Vec::new());
                for &l in &levels {
                    let (x, y) = d.select(|z| z == l);
                    let q = Profile::new(&x, &y);
                    let t = q.x_at_level(0.5 * q.y_range().1);
                    if t > 0.0 && t.is_finite() {
                        ls.push(l);
                        kobs.push(LN2 / t);
                        plateaus.push(q.y_range().1);
                    }
                }
                let (koff, kon) = line(&ls, &kobs);
                let koff = if koff > 0.0 { koff } else { kobs.iter().copied().fold(f64::INFINITY, f64::min) / 2.0 };
                let kon = if kon > 0.0 {
                    kon
                } else {
                    kobs.last().copied().unwrap_or(1.0) / levels.last().copied().unwrap_or(1.0)
                };
                let lmax = ls.last().copied().unwrap_or(1.0);
                let pmax = plateaus.last().copied().unwrap_or(ymax);
                vec![pmax * (lmax + koff / kon) / lmax, kon, koff]
            }
            T::Dissociation => {
                let ns = 0.9 * ymin;
                let t = p.x_at_level(0.5 * (y0 + ns)) - p.first_x();
                vec![y0, ns, if t > 0.0 { LN2 / t } else { rate_from_half(&p) }]
            }
            T::CompetitorKinetics => {
                let (l, k1, k2) = (c(0), c(1), c(2));
                let (x, y) = d.select(|z| z == 0.0);
                let q = Profile::new(&x, &y);
                let plateau = if q.points.is_empty() { ymax } else { q.y_range().1 };
                vec![plateau * (l + k2 / k1) / l, k1, k2]
            }
            T::MichaelisMenten | T::Allosteric => {
                let mut s = vec![1.2 * ymax, positive_or(half_level(&p, 0.5), p.median_x())];
                if self == T::Allosteric {
                    s.push(1.0);
                }
                s
            }
            T::SubstrateInhibition => {
                let km = positive_or(p.x_at_level(0.5 * ymax), p.median_x());
                vec![1.5 * ymax, km, 2.0 * p.last_x().max(km)]
            }
            T::Competitive | T::Uncompetitive | T::Noncompetitive | T::Mixed => {
                let zmin = d.z.iter().copied().filter(|v| v.is_finite()).fold(f64::INFINITY, f64::min);
                let (x, y) = d.select(|z| z == zmin);
                let q = Profile::new(&x, &y);
                let vmax = 1.2 * q.y_range().1;
                let km = positive_or(q.x_at_level(0.5 * q.y_range().1), q.median_x());
                let mut zs: Vec<f64> = d.z.iter().copied().filter(|v| *v > 0.0).collect();
                zs.sort_by(f64::total_cmp);
                let ki = zs.get(zs.len() / 2).copied().unwrap_or(1.0);
                let mut s = vec![vmax, km, ki];
                if self == T::Mixed {
                    s.push(1.0);
                }
                s
            }
            T::TightBinding => {
                let e = c(0);
                let half = p.x_at_level(0.5 * y0);
                vec![y0, positive_or(half - e / 2.0, half / 2.0).max(1e-12 * e.abs().max(1.0))]
            }
            T::ProgressCurve => {
                let s0 = c(0);
                let slope = p.initial_slope();
                vec![positive_or(slope * 1.5, s0 / (p.last_x() - p.first_x()).max(1e-12)), s0 / 2.0]
            }
            T::Agonist => vec![y0, y1, mid, 1.0],
            T::Inhibitor => vec![y0, y1, mid, 1.0],
            T::BellShaped => {
                let peak = p.extreme_index();
                let left = Profile { points: p.points[..=peak].to_vec() };
                let right = Profile { points: p.points[peak..].to_vec() };
                let top = p.points.get(peak).map_or(ymax, |q| q.1);
                vec![y0, top, y1, left.x_at_fraction(0.5), right.x_at_fraction(0.5), 1.0, 1.0]
            }
            T::Biphasic => vec![y0, y1, 0.5, mid - 1.0, mid + 1.0, 1.0, 1.0],
            T::GaddumSchild => {
                let zmin = d.z.iter().copied().filter(|v| v.is_finite()).fold(f64::INFINITY, f64::min);
                let zmax = d.z.iter().copied().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
                let (x0, yy0) = d.select(|z| z == zmin);
                let base = Profile::new(&x0, &yy0);
                let (x1, yy1) = d.select(|z| z == zmax);
                let shifted = Profile::new(&x1, &yy1);
                let m0 = base.x_at_fraction(0.5);
                let ratio = exp10(shifted.x_at_fraction(0.5) - m0);
                let pa2 = if ratio > 1.5 && zmax > 0.0 {
                    (ratio - 1.0).log10() - zmax.log10()
                } else {
                    -zmax.max(1e-12).log10()
                };
                vec![base.first_y(), base.last_y(), m0, 1.0, pa2, 1.0]
            }
            T::Operational => vec![ymin, 1.1 * ymax, 1.0, mid + 0.5, 0.5],
            T::RelativePotency => vec![y0, y1, mid, if y1 >= y0 { 1.0 } else { -1.0 }],
            T::Gompertz | T::Baranyi => {
                let (mu, xi, yi) = p.max_slope();
                let mu = positive_or(mu, 1.0);
                if self == T::Gompertz {
                    let lag = (xi - yi / mu).max(p.first_x());
                    vec![ymax, mu, lag]
                } else {
                    let lag = (xi - (yi - y0) / mu).max(p.first_x().max(0.0));
                    vec![y0, ymax, mu, positive_or(lag, 0.1 * (p.last_x() - p.first_x()))]
                }
            }
            T::OnePhaseDecay | T::OnePhaseAssociation => vec![y0, y1, rate_from_half(&p)],
            T::TwoPhaseDecay => {
                let k = rate_from_half(&p);
                let span = y0 - y1;
                vec![y1, span / 2.0, 3.0 * k, span / 2.0, k / 3.0]
            }
            T::PlateauDecay => {
                let drop = y0 - 0.1 * (y0 - y1);
                let x0 = p
                    .points
                    .iter()
                    .find(|q| if y0 > y1 { q.1 < drop } else { q.1 > drop })
                    .map_or(p.first_x(), |q| q.0);
                let after: Vec<(f64, f64)> = p.points.iter().copied().filter(|q| q.0 >= x0).collect();
                let q = Profile { points: after };
                let t = q.x_at_fraction(0.5) - x0;
                vec![y0, y1, x0, if t > 0.0 { LN2 / t } else { rate_from_half(&p) }]
            }
            T::LinearQuadratic => {
                let pe = if y0 > 0.0 { y0 } else { ymax };
                // ln(Y/PE) = −αD − βD² by least squares through the origin.
                let (mut sxx, mut sxq, mut sqq, mut sxy, mut sqy) = (0.0, 0.0, 0.0, 0.0, 0.0);
                for (&x, &y) in d.x.iter().zip(&d.y) {
                    if y > 0.0 {
                        let l = -(y / pe).ln();
                        let q = x * x;
                        sxx += x * x;
                        sxq += x * q;
                        sqq += q * q;
                        sxy += x * l;
                        sqy += q * l;
                    }
                }
                let det = sxx * sqq - sxq * sxq;
                let (a, b) = if det.abs() > 0.0 {
                    ((sxy * sqq - sqy * sxq) / det, (sqy * sxx - sxy * sxq) / det)
                } else {
                    (0.2, 0.02)
                };
                vec![pe, a.max(1e-3), b.max(1e-4)]
            }
            T::Boltzmann => vec![y0, y1, mid, (p.last_x() - p.first_x()) / 10.0],
            T::Arrhenius => {
                let pairs: Vec<(f64, f64)> =
                    d.x.iter()
                        .zip(&d.y)
                        .filter(|(x, y)| **x > 0.0 && **y > 0.0)
                        .map(|(x, y)| (1.0 / x, y.ln()))
                        .collect();
                let (u, v): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
                let (a, b) = line(&u, &v);
                vec![a, -b * GAS_CONSTANT / 1000.0]
            }
        }
    }
}

/// `D` such that `αD + βD² = −ln(sf)`.
fn dose_for_survival(alpha: f64, beta: f64, sf: f64) -> f64 {
    let l = -sf.ln();
    if beta.abs() < 1e-300 {
        return l / alpha;
    }
    (-alpha + (alpha * alpha + 4.0 * beta * l).sqrt()) / (2.0 * beta)
}

fn positive_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() && v > 0.0 {
        v
    } else if fallback.is_finite() && fallback > 0.0 {
        fallback
    } else {
        1.0
    }
}

/// A number literal the formula language reads as a float.
fn lit(x: f64) -> String {
    let t = crate::script_text::number(x);
    if t.chars().all(|c| c.is_ascii_digit() || c == '-') { format!("{t}.0") } else { t }
}

/// Least-squares line `y = a + b·x`; `(NaN, NaN)` with fewer than two
/// distinct x values.
fn line(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    if x.len() < 2 {
        return (f64::NAN, f64::NAN);
    }
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let sxx: f64 = x.iter().map(|v| (v - mx).powi(2)).sum();
    if sxx == 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let sxy: f64 = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let b = sxy / sxx;
    (my - b * mx, b)
}

/// Slope of `y = b·x`.
fn through_origin(x: &[f64], y: &[f64]) -> f64 {
    let sxx: f64 = x.iter().map(|v| v * v).sum();
    if sxx == 0.0 { 0.0 } else { x.iter().zip(y).map(|(a, b)| a * b).sum::<f64>() / sxx }
}

/// The data starting values are computed from.
#[derive(Debug, Clone, Default)]
pub struct StartData {
    /// x of each usable row.
    pub x: Vec<f64>,
    /// Response of each usable row.
    pub y: Vec<f64>,
    /// Second column of each usable row (NaN when none).
    pub z: Vec<f64>,
    /// Values of [`CurveTemplate::all_constants`].
    pub constants: Vec<f64>,
}

impl StartData {
    /// The x and y of rows whose second column satisfies `keep`.
    fn select(&self, keep: impl Fn(f64) -> bool) -> (Vec<f64>, Vec<f64>) {
        self.x.iter().zip(&self.y).zip(&self.z).filter(|(_, z)| keep(**z)).map(|((x, y), _)| (*x, *y)).unzip()
    }
}

/// Mean response at each distinct x, in increasing x.
#[derive(Debug, Clone)]
struct Profile {
    points: Vec<(f64, f64)>,
}

impl Profile {
    fn new(x: &[f64], y: &[f64]) -> Self {
        let mut pairs: Vec<(f64, f64)> =
            x.iter().zip(y).filter(|(a, b)| a.is_finite() && b.is_finite()).map(|(a, b)| (*a, *b)).collect();
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut points: Vec<(f64, f64, usize)> = Vec::new();
        for (a, b) in pairs {
            match points.last_mut() {
                Some(last) if last.0 == a => {
                    last.1 += b;
                    last.2 += 1;
                }
                _ => points.push((a, b, 1)),
            }
        }
        Self { points: points.into_iter().map(|(a, s, n)| (a, s / n as f64)).collect() }
    }
    fn first_x(&self) -> f64 {
        self.points.first().map_or(f64::NAN, |p| p.0)
    }
    fn last_x(&self) -> f64 {
        self.points.last().map_or(f64::NAN, |p| p.0)
    }
    fn first_y(&self) -> f64 {
        self.points.first().map_or(f64::NAN, |p| p.1)
    }
    fn last_y(&self) -> f64 {
        self.points.last().map_or(f64::NAN, |p| p.1)
    }
    fn y_range(&self) -> (f64, f64) {
        self.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.1), b.max(p.1)))
    }
    fn median_x(&self) -> f64 {
        self.points.get(self.points.len() / 2).map_or(f64::NAN, |p| p.0)
    }
    /// Index of the point farthest from the line through the two ends.
    fn extreme_index(&self) -> usize {
        let (a, b) = (self.first_y(), self.last_y());
        let n = self.points.len();
        (0..n)
            .max_by(|&i, &j| {
                let base = |k: usize| a + (b - a) * k as f64 / (n.max(2) - 1) as f64;
                (self.points[i].1 - base(i)).abs().total_cmp(&(self.points[j].1 - base(j)).abs())
            })
            .unwrap_or(0)
    }
    /// The first x where the curve crosses `level`, by linear interpolation;
    /// the median x when it never does.
    fn x_at_level(&self, level: f64) -> f64 {
        for w in self.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.1 - level) * (b.1 - level) <= 0.0 && a.1 != b.1 {
                return a.0 + (level - a.1) / (b.1 - a.1) * (b.0 - a.0);
            }
        }
        self.median_x()
    }
    /// x where the curve is `frac` of the way from its first to its last
    /// value.
    fn x_at_fraction(&self, frac: f64) -> f64 {
        self.x_at_level(self.first_y() + frac * (self.last_y() - self.first_y()))
    }
    /// As [`Self::x_at_fraction`], interpolating in log x (positive x only).
    fn x_at_fraction_log(&self, frac: f64) -> Option<f64> {
        let pts: Vec<(f64, f64)> = self.points.iter().copied().filter(|p| p.0 > 0.0).map(|p| (p.0.ln(), p.1)).collect();
        if pts.len() < 2 {
            return None;
        }
        let q = Self { points: pts };
        Some(q.x_at_level(self.first_y() + frac * (self.last_y() - self.first_y())).exp())
    }
    /// Steepest slope between neighbouring points, with the midpoint.
    fn max_slope(&self) -> (f64, f64, f64) {
        let mut best = (f64::NAN, f64::NAN, f64::NAN);
        for w in self.points.windows(2) {
            let s = (w[1].1 - w[0].1) / (w[1].0 - w[0].0);
            if best.0.is_nan() || s > best.0 {
                best = (s, 0.5 * (w[0].0 + w[1].0), 0.5 * (w[0].1 + w[1].1));
            }
        }
        best
    }
    /// Slope of the first three points.
    fn initial_slope(&self) -> f64 {
        let k = self.points.len().min(3);
        let (x, y): (Vec<f64>, Vec<f64>) = self.points[..k].iter().copied().unzip();
        line(&x, &y).1
    }
}

/// A template applied to a table: what the report needs beyond the model.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateSettings {
    /// The template.
    pub template: CurveTemplate,
    /// x column.
    pub x: usize,
    /// Second column, for templates that read one.
    pub second: Option<usize>,
    /// Dilution-factor column (standard curves): sample concentrations are
    /// multiplied by it.
    pub dilution: Option<usize>,
    /// Values of [`CurveTemplate::all_constants`] (NaN when not given).
    pub constants: Vec<f64>,
    /// Precision-profile limit for the quantitation range, % CV.
    pub cv_limit: f64,
    /// Back-calculation limits for the standards.
    pub acceptance: AcceptanceLimits,
    /// Data set that is the standard preparation (relative potency); the
    /// first data set when `None`.
    pub standard: Option<String>,
}

impl TemplateSettings {
    /// The template on `x` with its default constants and settings.
    #[must_use]
    pub fn new(template: CurveTemplate, x: usize) -> Self {
        Self {
            template,
            x,
            second: None,
            dilution: None,
            constants: template.all_constants().iter().map(|c| c.default).collect(),
            cv_limit: 20.0,
            acceptance: AcceptanceLimits::default(),
            standard: None,
        }
    }

    /// Every column the settings read.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.x];
        c.extend(self.second);
        c.extend(self.dilution);
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.x = f(self.x);
        self.second = self.second.map(f);
        self.dilution = self.dilution.map(f);
    }

    /// The expression for the second column in the formula: the column
    /// itself, or for an indicator an expression that is 1 for total binding.
    fn second_expression(&self, table: &DataTable) -> Result<String> {
        let Some(role) = self.template.second() else { return Ok(String::new()) };
        let j = self.second.ok_or_else(|| invalid(format!("choose the {} column", role.label.to_lowercase())))?;
        let col = table.column(j).ok_or_else(|| invalid(format!("no column {j}")))?;
        let reference = column_reference(&col.name);
        match role.kind {
            SecondKind::Numeric => {
                if col.as_numeric().is_none() {
                    return Err(invalid(format!("{} must be numeric", col.name)));
                }
                Ok(reference)
            }
            SecondKind::Indicator => Ok(match col.as_numeric() {
                Some(_) => format!("(if {reference} != 0.0 {{ 1.0 }} else {{ 0.0 }})"),
                None => {
                    let total = total_label(col);
                    format!("(if {reference} == {total:?} {{ 1.0 }} else {{ 0.0 }})")
                }
            }),
        }
    }

    /// The second column's value in each row for the starting values: the
    /// number, or 1/0 for an indicator.
    fn second_values(&self, table: &DataTable) -> Vec<f64> {
        let n = table.n_rows();
        let (Some(role), Some(col)) = (self.template.second(), self.second.and_then(|j| table.column(j))) else {
            return vec![f64::NAN; n];
        };
        match (role.kind, col.as_numeric()) {
            (SecondKind::Numeric, Some(v)) => v.to_vec(),
            (SecondKind::Indicator, Some(v)) => v
                .iter()
                .map(|x| {
                    if x.is_nan() {
                        f64::NAN
                    } else if *x != 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect(),
            (SecondKind::Indicator, None) => {
                let total = total_label(col);
                (0..n)
                    .map(|r| {
                        let t = col.level_text(r);
                        if t.is_empty() {
                            f64::NAN
                        } else if t == total {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .collect()
            }
            (SecondKind::Numeric, None) => vec![f64::NAN; n],
        }
    }

    /// The nonlinear regression of `response` this template describes, with
    /// starting values from the table and the template's default bounds,
    /// sharing and weighting. `group` is the data-set column.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a required column or constant not
    /// given, or no rows with both x and the response.
    pub fn request(&self, table: &DataTable, response: usize, group: Option<usize>) -> Result<NonlinearRequest> {
        let t = self.template;
        let numeric = |j: usize| -> Result<&[f64]> {
            let c = table.column(j).ok_or_else(|| invalid(format!("no column {j}")))?;
            c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))
        };
        let y = numeric(response)?;
        let x = numeric(self.x)?;
        if self.x == response {
            return Err(invalid("the response and x must be different columns"));
        }
        if let DataSets::Required(label) = t.data_sets() {
            if group.is_none() {
                return Err(invalid(format!("choose the {} column", label.to_lowercase())));
            }
        }
        let specs = t.all_constants();
        if self.constants.len() != specs.len() {
            return Err(invalid(format!("the {} template takes {} constants", t.label(), specs.len())));
        }
        for (spec, v) in specs.iter().zip(&self.constants) {
            if spec.required && !v.is_finite() {
                return Err(invalid(format!("enter the {}", spec.label)));
            }
        }
        let z_expr = self.second_expression(table)?;
        let x_ref = table.column(self.x).map(|c| column_reference(&c.name)).unwrap_or_default();
        let formula = t.formula(&x_ref, &z_expr, &self.constants);
        let z = self.second_values(table);
        let rows: Vec<usize> = (0..table.n_rows())
            .filter(|&r| x[r].is_finite() && y[r].is_finite() && (t.second().is_none() || z[r].is_finite()))
            .collect();
        if rows.is_empty() {
            return Err(invalid("no rows with both x and the response"));
        }
        let data = StartData {
            x: rows.iter().map(|&r| x[r]).collect(),
            y: rows.iter().map(|&r| y[r]).collect(),
            z: rows.iter().map(|&r| z[r]).collect(),
            constants: self.constants.clone(),
        };
        let starts = t.starts(&data);
        let bound = |v: f64| v.is_finite().then_some(v);
        let mut request = NonlinearRequest::formula(table, response, &formula, &[]);
        request.parameters = t
            .parameters()
            .iter()
            .zip(&starts)
            .map(|(p, &s)| {
                let s = if s.is_finite() { s } else { 1.0 };
                // Keep the start strictly inside the bounds.
                let s = if p.lower.is_finite() && s <= p.lower {
                    if p.upper.is_finite() { 0.5 * (p.lower + p.upper) } else { p.lower + s.abs().max(1e-6) }
                } else if p.upper.is_finite() && s >= p.upper {
                    if p.lower.is_finite() { 0.5 * (p.lower + p.upper) } else { p.upper - s.abs().max(1e-6) }
                } else {
                    s
                };
                ParameterSetting {
                    name: p.name.to_owned(),
                    start: Some(s),
                    lower: bound(p.lower),
                    upper: bound(p.upper),
                    shared: p.shared,
                }
            })
            .collect();
        request.group = group;
        request.weighting = t.default_weighting();
        request.template = Some(Box::new(self.clone()));
        Ok(request)
    }
}

/// The label of an indicator column that means total binding: the first
/// whose text contains "total" (any case), else the first in the column.
fn total_label(col: &stat_data::Column) -> String {
    let n = col.data.len();
    let mut first = None;
    for r in 0..n {
        let t = col.level_text(r);
        if t.is_empty() {
            continue;
        }
        if t.to_lowercase().contains("total") {
            return t;
        }
        first.get_or_insert(t);
    }
    first.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        let mut keys = std::collections::HashSet::new();
        for t in CurveTemplate::ALL {
            assert!(keys.insert(t.key()), "duplicate key {}", t.key());
            assert_eq!(CurveTemplate::from_key(t.key()), Some(t));
            for p in t.parameters() {
                assert!(crate::formula::is_identifier(p.name), "{}: {}", t.key(), p.name);
            }
            for d in t.derived() {
                assert!(d.single.is_none_or(|k| k < t.parameters().len()), "{}", d.label);
                assert!(d.needs.iter().all(|&i| i < t.all_constants().len()), "{}", d.label);
            }
        }
        let grouped: usize = TemplateGroup::ALL.iter().map(|&g| CurveTemplate::of_group(g).count()).sum();
        assert_eq!(grouped, CurveTemplate::ALL.len());
    }

    #[test]
    fn helpers() {
        assert!(
            (dose_for_survival(0.2, 0.02, 0.1) - {
                let l = 10f64.ln();
                (-0.2 + (0.04 + 0.08 * l).sqrt()) / 0.04
            })
            .abs()
                < 1e-12
        );
        let p = Profile::new(&[3.0, 1.0, 2.0, 2.0], &[30.0, 10.0, 18.0, 22.0]);
        assert_eq!(p.points, vec![(1.0, 10.0), (2.0, 20.0), (3.0, 30.0)]);
        assert!((p.x_at_level(25.0) - 2.5).abs() < 1e-12);
        assert!((p.x_at_fraction(0.5) - 2.0).abs() < 1e-12);
        let (a, b) = line(&[1.0, 2.0, 3.0], &[3.0, 5.0, 7.0]);
        assert!((a - 1.0).abs() < 1e-12 && (b - 2.0).abs() < 1e-12);
        assert_eq!(lit(2.0), "2.0");
    }
}

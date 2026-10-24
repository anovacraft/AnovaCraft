//! Image spectrum report: the image, its centred log-magnitude, phase and
//! power spectra, the radially averaged power spectrum with a power-law
//! fit, the angular (orientation) spectrum and the dominant spatial
//! frequencies.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::image2d::{
    SpectrumOptions2, Taper2, angular_spectrum, directional_profile, dominant_frequencies, power_law_fit, radial_spectrum, spectrum2,
};
use stat_signal::window::Window;

use super::{
    ImageInput, Images, PaddingChoice, count, decibels, flag, frequency_section, invalid, number, section, summary_table,
    text,
};
use crate::format::format_number;
use crate::signal_analysis::MAX_PLOTTED_GROUPS;
use stat_viz::{MarkShape, RasterMark};
use crate::project::s;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// The data window applied before the transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaperChoice {
    /// No window: edges of the image show as a cross through the centre.
    None,
    /// Hann window across and down (outer product).
    #[default]
    SeparableHann,
    /// Hann window swept around the centre (no preferred direction).
    RadialHann,
}

impl TaperChoice {
    /// All choices in menu order.
    pub const ALL: [Self; 3] = [Self::None, Self::SeparableHann, Self::RadialHann];

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SeparableHann => "separable-hann",
            Self::RadialHann => "radial-hann",
        }
    }

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::SeparableHann => "Hann across and down",
            Self::RadialHann => "Radial Hann",
        }
    }

    /// The engine window.
    #[must_use]
    pub fn taper(self) -> Taper2 {
        match self {
            Self::None => Taper2::None,
            Self::SeparableHann => Taper2::Separable(Window::Hann),
            Self::RadialHann => Taper2::Radial(Window::Hann),
        }
    }
}

/// Settings of an image spectrum report.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageSpectrumSettings {
    /// Subtract the image mean first (default true).
    pub remove_mean: bool,
    /// Window (default Hann across and down).
    pub taper: TaperChoice,
    /// Padding (default none).
    pub padding: PaddingChoice,
    /// Show the phase spectrum (default false).
    pub phase: bool,
    /// Show the radially averaged power spectrum and its power-law fit
    /// (default true).
    pub radial: bool,
    /// Show the angular spectrum (default true).
    pub angular: bool,
    /// Show radial profiles: cuts through the spectrum along directions
    /// (default true).
    pub profiles: bool,
    /// Directions of the radial profiles in degrees, counter-clockwise
    /// from horizontal as seen on screen (default 0 and 90: across and up
    /// the image).
    pub profile_directions: Vec<f64>,
    /// Also profile along each dominant frequency's direction (default
    /// false).
    pub profile_peaks: bool,
    /// Half-width in degrees of the wedge averaged around each direction
    /// (default 0: a straight cut).
    pub profile_half_width: f64,
    /// Direction sectors of the angular spectrum over 0–180° (default 36).
    pub sectors: usize,
    /// Dominant frequencies listed (default 6; 0 for none).
    pub peaks: usize,
    /// Lowest radial frequency considered for peaks and fits, as a fraction
    /// of the Nyquist frequency (default 0.02, which skips slow shading).
    pub min_frequency_fraction: f64,
}

impl Default for ImageSpectrumSettings {
    fn default() -> Self {
        Self {
            remove_mean: true,
            taper: TaperChoice::SeparableHann,
            padding: PaddingChoice::None,
            phase: false,
            radial: true,
            angular: true,
            profiles: true,
            profile_directions: vec![0.0, 90.0],
            profile_peaks: false,
            profile_half_width: 0.0,
            sectors: 36,
            peaks: 6,
            min_frequency_fraction: 0.02,
        }
    }
}

impl ImageSpectrumSettings {
    pub(super) fn json_members(&self) -> Vec<(String, JsonValue)> {
        let n = JsonValue::number;
        vec![
            ("remove_mean".into(), JsonValue::Bool(self.remove_mean)),
            ("window".into(), s(self.taper.key())),
            ("padding".into(), s(self.padding.key())),
            ("phase".into(), JsonValue::Bool(self.phase)),
            ("radial".into(), JsonValue::Bool(self.radial)),
            ("angular".into(), JsonValue::Bool(self.angular)),
            ("profiles".into(), JsonValue::Bool(self.profiles)),
            ("profile_directions".into(), JsonValue::Array(self.profile_directions.iter().map(|&d| n(d)).collect())),
            ("profile_peaks".into(), JsonValue::Bool(self.profile_peaks)),
            ("profile_half_width".into(), n(self.profile_half_width)),
            ("sectors".into(), n(self.sectors as f64)),
            ("peaks".into(), n(self.peaks as f64)),
            ("min_frequency_fraction".into(), n(self.min_frequency_fraction)),
        ]
    }

    pub(super) fn from_json(v: &JsonValue) -> Result<Self> {
        let d = Self::default();
        let taper = match text(v, "window") {
            None => d.taper,
            Some(k) => TaperChoice::ALL
                .into_iter()
                .find(|t| t.key() == k)
                .ok_or_else(|| invalid(format!("unknown image window {k:?}")))?,
        };
        Ok(Self {
            remove_mean: flag(v, "remove_mean", d.remove_mean),
            taper,
            padding: PaddingChoice::from_key(text(v, "padding"))?,
            phase: flag(v, "phase", d.phase),
            radial: flag(v, "radial", d.radial),
            angular: flag(v, "angular", d.angular),
            profiles: flag(v, "profiles", d.profiles),
            profile_directions: match v.get("profile_directions") {
                Some(JsonValue::Array(list)) => list.iter().filter_map(JsonValue::as_f64).collect(),
                _ => d.profile_directions,
            },
            profile_peaks: flag(v, "profile_peaks", d.profile_peaks),
            profile_half_width: number(v, "profile_half_width", d.profile_half_width),
            sectors: count(v, "sectors", d.sectors),
            peaks: count(v, "peaks", d.peaks),
            min_frequency_fraction: number(v, "min_frequency_fraction", d.min_frequency_fraction),
        })
    }

    fn check(&self) -> Result<()> {
        if !(2..=720).contains(&self.sectors) {
            return Err(invalid("use between 2 and 720 direction sectors"));
        }
        if !(0.0..1.0).contains(&self.min_frequency_fraction) {
            return Err(invalid("the lowest frequency must be a fraction of the Nyquist frequency below 1"));
        }
        if self.profiles {
            if self.profile_directions.iter().any(|d| !d.is_finite()) {
                return Err(invalid("each profile direction must be a number of degrees"));
            }
            if self.profile_directions.len() > 12 {
                return Err(invalid("draw at most 12 profile directions"));
            }
            if self.profile_directions.is_empty() && !(self.profile_peaks && self.peaks > 0) {
                return Err(invalid("give at least one profile direction, or profile the dominant frequencies"));
            }
            if !(0.0..=90.0).contains(&self.profile_half_width) {
                return Err(invalid("the wedge half-width must be between 0 and 90 degrees"));
            }
        }
        if self.peaks > 50 {
            return Err(invalid("list at most 50 dominant frequencies"));
        }
        Ok(())
    }
}

/// Labelled log₁₀ ticks over `[lo, hi]` (log coordinates): 1, 2, 5 × 10ⁿ,
/// or only the powers of ten when the range spans more than four decades
/// (so the labels do not crowd).
pub(crate) fn log_ticks(lo: f64, hi: f64) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    if !(lo.is_finite() && hi.is_finite()) {
        return out;
    }
    let mantissas: &[f64] = if hi - lo > 4.0 { &[1.0] } else { &[1.0, 2.0, 5.0] };
    for e in (lo.floor() as i32)..=(hi.ceil() as i32) {
        for &m in mantissas {
            let v = m * 10f64.powi(e);
            let l = v.log10();
            if l >= lo - 1e-9 && l <= hi + 1e-9 {
                out.push((l, format_number(v, 3)));
            }
        }
    }
    out
}

/// Marks for the dominant-frequency peaks (both members of each pair).
fn peak_marks(peaks: &[stat_signal::image2d::FrequencyPeak]) -> Vec<RasterMark> {
    let mut out = Vec::new();
    for (k, p) in peaks.iter().enumerate() {
        for sign in [1.0, -1.0] {
            out.push(RasterMark { at: [sign * p.fx, sign * p.fy], label: (k + 1).to_string(), shape: MarkShape::Circle });
        }
    }
    out
}

pub(super) fn run(
    table: &DataTable,
    states: &RowStateManager,
    input: &ImageInput,
    settings: &ImageSpectrumSettings,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    settings.check()?;
    let images = Images::extract(table, states, input, 4)?;
    let unit = images.frequency_unit();
    let mut sections = Vec::new();
    let mut summary = images.summary_rows();
    summary.push(("Mean removed".into(), if settings.remove_mean { "Yes" } else { "No" }.into()));
    summary.push(("Window".into(), settings.taper.label().into()));
    summary.push(("Padding".into(), settings.padding.label().into()));
    sections.push(section(SectionKind::ImageSummary, "Summary", SectionContent::Table(summary_table(summary))));

    let mut peak_table = TextTable::new(
        ["Image", "Peak", "Horizontal frequency", "Vertical frequency", "Frequency", "Period", "Direction (°)", "Amplitude", "Share of power"]
            .into_iter()
            .skip(usize::from(images.group_name.is_none())),
    );
    let mut fit_table = TextTable::new(
        ["Image", "Slope (log–log)", "Std error", "Exponent β (P ∝ f^−β)", "R²", "Frequencies used"]
            .into_iter()
            .skip(usize::from(images.group_name.is_none())),
    );
    let mut radial_curves: Vec<Polyline> = Vec::new();
    let mut angular_curves: Vec<Polyline> = Vec::new();
    let wedge = settings.profile_half_width > 0.0;
    let mut profile_curves: Vec<(String, Polyline)> = Vec::new();
    let mut profile_table = TextTable::new(
        ["Image", "Direction (°)", "Frequency", "Period", "Power", "Cells averaged"]
            .into_iter()
            .take(if wedge { 6 } else { 5 })
            .skip(usize::from(images.group_name.is_none())),
    );
    let mut radial_range = (f64::INFINITY, f64::NEG_INFINITY);
    for (i, im) in images.images.iter().enumerate() {
        let options = SpectrumOptions2 {
            remove_mean: settings.remove_mean,
            taper: settings.taper.taper(),
            padding: settings.padding.padding(im.grid.rows, im.grid.cols),
        };
        let spec = spectrum2(&im.grid, &options)?;
        let nyquist = (0.5 / im.grid.dx).min(0.5 / im.grid.dy);
        let fmin = settings.min_frequency_fraction * nyquist;
        let peaks = if settings.peaks > 0 { dominant_frequencies(&spec, settings.peaks, fmin) } else { Vec::new() };
        let suffix = images.suffix(i);
        if i < images.drawn() {
            sections.push(images.image_section(i, SectionKind::ImagePlot, "Image", &im.grid, &images.name));
            let mut db = spec.magnitude.clone();
            db.data = decibels(&spec.magnitude.data.iter().map(|m| m * m).collect::<Vec<_>>());
            sections.push(frequency_section(
                SectionKind::ImageSpectrumMagnitude,
                &format!("Log-Magnitude Spectrum{suffix}"),
                &db,
                &unit,
                "Magnitude (dB)",
                peak_marks(&peaks),
            ));
            if settings.phase {
                sections.push(frequency_section(
                    SectionKind::ImageSpectrumPhase,
                    &format!("Phase Spectrum{suffix}"),
                    &spec.phase,
                    &unit,
                    "Phase (radians)",
                    Vec::new(),
                ));
            }
        }
        let label = im.label.clone();
        let lead = |row: Vec<String>| -> Vec<String> {
            if images.group_name.is_some() { std::iter::once(label.clone()).chain(row).collect() } else { row }
        };
        for (k, p) in peaks.iter().enumerate() {
            peak_table.push(lead(vec![
                (k + 1).to_string(),
                format_number(p.fx, 6),
                format_number(p.fy, 6),
                format_number(p.frequency, 6),
                format_number(p.period, 6),
                format_number(p.direction, 4),
                format_number(p.amplitude, 6),
                format_number(p.share, 4),
            ]));
        }
        if settings.radial {
            let radial = radial_spectrum(&spec);
            let curve: Polyline = radial
                .frequency
                .iter()
                .zip(&radial.power)
                .filter(|(f, p)| **f > 0.0 && **p > 0.0)
                .map(|(f, p)| [f.log10(), p.log10()])
                .collect();
            for p in &curve {
                radial_range = (radial_range.0.min(p[0]), radial_range.1.max(p[0]));
            }
            radial_curves.push(curve);
            match power_law_fit(&radial, Some(fmin.max(f64::MIN_POSITIVE)), Some(nyquist)) {
                Ok(fit) => fit_table.push(lead(vec![
                    format_number(fit.slope, 5),
                    format_number(fit.slope_se, 4),
                    format_number(fit.exponent, 5),
                    format_number(fit.r_squared, 4),
                    fit.points.to_string(),
                ])),
                Err(e) => fit_table.push(lead(vec![format!("not fitted: {e}"), String::new(), String::new(), String::new(), String::new()])),
            }
        }
        if settings.angular {
            let a = angular_spectrum(&spec, settings.sectors, Some(fmin), Some(nyquist))?;
            angular_curves.push(a.direction.iter().zip(&a.power).map(|(&d, &p)| [d, p]).collect());
        }
        if settings.profiles {
            let mut directions: Vec<f64> = settings.profile_directions.iter().map(|d| d.rem_euclid(180.0)).collect();
            if settings.profile_peaks {
                directions.extend(peaks.iter().map(|p| p.direction));
            }
            let mut seen: Vec<f64> = Vec::new();
            directions.retain(|&d| {
                let new = seen.iter().all(|&e| (e - d).abs() > 0.05);
                seen.push(d);
                new
            });
            for d in directions {
                let profile = directional_profile(&spec, d, settings.profile_half_width)?;
                let name = format!("{}°", format_number(profile.direction, 4));
                let curve_name = if images.group_name.is_some() { format!("{} · {name}", im.label) } else { name.clone() };
                let curve: Polyline = profile
                    .frequency
                    .iter()
                    .zip(&profile.power)
                    .filter(|(_, p)| **p > 0.0)
                    .map(|(&f, &p)| [f, p.log10()])
                    .collect();
                profile_curves.push((curve_name, curve));
                for k in 0..profile.frequency.len() {
                    let f = profile.frequency[k];
                    let mut row = vec![
                        format_number(profile.direction, 4),
                        format_number(f, 6),
                        if f > 0.0 { format_number(1.0 / f, 6) } else { String::new() },
                        format_number(profile.power[k], 6),
                    ];
                    if wedge {
                        row.push(profile.count[k].to_string());
                    }
                    profile_table.push(lead(row));
                }
            }
        }
    }
    let curve_plot = |curves: Vec<Polyline>, x: &str, y: &str, name: &str| images.curves(curves, x, y, name);
    if settings.radial {
        let mut p = curve_plot(
            radial_curves,
            &format!("Radial frequency ({unit})"),
            "Mean power",
            "Radially averaged power",
        );
        p.x_ticks = Some(log_ticks(radial_range.0, radial_range.1));
        let (lo, hi) = p
            .groups
            .as_ref()
            .map_or_else(|| p.line.iter().flatten().map(|q| q[1]).collect::<Vec<_>>(), |g| g.lines.iter().flatten().map(|q| q[1]).collect())
            .into_iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        p.y_ticks = Some(log_ticks(lo, hi));
        sections.push(section(SectionKind::ImageRadialSpectrum, "Radially Averaged Power Spectrum", SectionContent::Plot(Box::new(p))));
        sections.push(section(SectionKind::ImagePowerLaw, "Power-Law Fit of the Radial Spectrum", SectionContent::Table(fit_table)));
    }
    if settings.angular {
        let mut p = curve_plot(angular_curves, "Direction of the wave (degrees from horizontal)", "Mean power", "Angular power");
        p.x_ticks = Some((0..=6).map(|k| (f64::from(k) * 30.0, format!("{}°", k * 30))).collect());
        sections.push(section(SectionKind::ImageAngularSpectrum, "Angular Spectrum", SectionContent::Plot(Box::new(p))));
    }
    if settings.profiles && !profile_curves.is_empty() {
        let (lo, hi) = profile_curves
            .iter()
            .flat_map(|(_, c)| c.iter().map(|q| q[1]))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let k = profile_curves.len().min(MAX_PLOTTED_GROUPS);
        let (labels, lines): (Vec<String>, Vec<Polyline>) = profile_curves.into_iter().take(k).unzip();
        let p = PlotSpec {
            x_label: format!("Frequency along the direction ({unit})"),
            y_label: if wedge { "Mean power" } else { "Power" }.to_owned(),
            groups: Some(PointGroups { title: "Direction".into(), labels, membership: Vec::new(), lines }),
            y_ticks: Some(log_ticks(lo, hi)),
            ..PlotSpec::default()
        };
        sections.push(section(SectionKind::ImageRadialProfile, "Radial Profiles", SectionContent::Plot(Box::new(p))));
        sections.push(section(SectionKind::ImageRadialProfileValues, "Radial Profile Values", SectionContent::Table(profile_table)));
    }
    if settings.peaks > 0 {
        if peak_table.rows.is_empty() {
            sections.push(section(
                SectionKind::ImageDominantFrequencies,
                "Dominant Frequencies",
                SectionContent::Note("No peaks above the lowest frequency considered.".into()),
            ));
        } else {
            sections.push(section(SectionKind::ImageDominantFrequencies, "Dominant Frequencies", SectionContent::Table(peak_table)));
        }
    }
    Ok((format!("Image Spectrum: {}", images.name), sections, Vec::new()))
}

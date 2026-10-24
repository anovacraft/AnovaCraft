//! Image frequency filter report: a low-, high-, band-pass or band-reject
//! mask and notch filters applied by inverse FFT, with the filtered image,
//! the part removed, the mask and the spectrum with the notches marked.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::filter::Band;
use stat_signal::image2d::{
    FilterShape, FrequencyFilter, Notch, SpectrumOptions2, Taper2, dominant_frequencies, frequency_filter, spectrum2,
};
use stat_signal::window::Window;

use super::{
    ImageInput, Images, PaddingChoice, count, decibels, flag, frequency_section, invalid, number, section, summary_table,
    text,
};
use crate::format::format_number;
use stat_viz::{MarkShape, RasterMark};
use crate::project::s;
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;

/// The band of frequencies a filter keeps or rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BandChoice {
    /// No band filter (notches only).
    None,
    /// Keep frequencies below the cut-off (smooths).
    #[default]
    Lowpass,
    /// Keep frequencies above the cut-off (sharpens edges, removes shading).
    Highpass,
    /// Keep frequencies between the two edges.
    Bandpass,
    /// Remove frequencies between the two edges.
    Bandreject,
}

impl BandChoice {
    /// All choices in menu order.
    pub const ALL: [Self; 5] = [Self::None, Self::Lowpass, Self::Highpass, Self::Bandpass, Self::Bandreject];

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lowpass => "lowpass",
            Self::Highpass => "highpass",
            Self::Bandpass => "bandpass",
            Self::Bandreject => "bandreject",
        }
    }

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None (notches only)",
            Self::Lowpass => "Low-pass",
            Self::Highpass => "High-pass",
            Self::Bandpass => "Band-pass",
            Self::Bandreject => "Band-reject",
        }
    }

    /// Whether the band has two edges.
    #[must_use]
    pub fn two_edges(self) -> bool {
        matches!(self, Self::Bandpass | Self::Bandreject)
    }
}

/// The transition shape of a mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeChoice {
    /// Sharp cut-off (rings around edges in the image).
    Ideal,
    /// Butterworth roll-off of the settings' order.
    #[default]
    Butterworth,
    /// Gaussian roll-off (no ringing).
    Gaussian,
}

impl ShapeChoice {
    /// All choices in menu order.
    pub const ALL: [Self; 3] = [Self::Ideal, Self::Butterworth, Self::Gaussian];

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Ideal => "ideal",
            Self::Butterworth => "butterworth",
            Self::Gaussian => "gaussian",
        }
    }

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ideal => "Ideal (sharp)",
            Self::Butterworth => "Butterworth",
            Self::Gaussian => "Gaussian",
        }
    }

    fn shape(self, order: f64) -> FilterShape {
        match self {
            Self::Ideal => FilterShape::Ideal,
            Self::Butterworth => FilterShape::Butterworth { order },
            Self::Gaussian => FilterShape::Gaussian,
        }
    }

    fn from_key(key: Option<&str>, default: Self) -> Result<Self> {
        match key {
            None => Ok(default),
            Some(k) => Self::ALL.into_iter().find(|c| c.key() == k).ok_or_else(|| invalid(format!("unknown filter shape {k:?}"))),
        }
    }
}

/// A notch at spatial frequency `(fx, fy)` (its conjugate is notched too).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotchPoint {
    /// Horizontal frequency.
    pub fx: f64,
    /// Vertical frequency.
    pub fy: f64,
}

/// Settings of an image frequency filter report.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageFilterSettings {
    /// The band filter (default low-pass).
    pub band: BandChoice,
    /// Cut-off of a low- or high-pass filter, or the lower edge of a band, in
    /// cycles per unit of x (`None`: a quarter of the Nyquist frequency).
    pub low: Option<f64>,
    /// Upper edge of a band (`None`: half the Nyquist frequency).
    pub high: Option<f64>,
    /// Shape of the band filter (default Butterworth).
    pub shape: ShapeChoice,
    /// Butterworth order (default 2).
    pub order: f64,
    /// Notches at given frequencies.
    pub notches: Vec<NotchPoint>,
    /// Also notch this many of the strongest spatial frequencies (default 0).
    pub auto_notches: usize,
    /// Radius of each notch in frequency units (`None`: two frequency
    /// steps of the image, the coarser of across and down).
    pub notch_radius: Option<f64>,
    /// Shape of the notches (default Gaussian).
    pub notch_shape: ShapeChoice,
    /// Keep only the notched frequencies instead of removing them (default false).
    pub notch_pass: bool,
    /// Padding (default mirror, which avoids ringing at the borders).
    pub padding: PaddingChoice,
}

impl Default for ImageFilterSettings {
    fn default() -> Self {
        Self {
            band: BandChoice::Lowpass,
            low: None,
            high: None,
            shape: ShapeChoice::Butterworth,
            order: 2.0,
            notches: Vec::new(),
            auto_notches: 0,
            notch_radius: None,
            notch_shape: ShapeChoice::Gaussian,
            notch_pass: false,
            padding: PaddingChoice::Mirror,
        }
    }
}

impl ImageFilterSettings {
    /// Notch filtering only: the `k` strongest frequencies.
    #[must_use]
    pub fn auto_notch(k: usize) -> Self {
        Self { band: BandChoice::None, auto_notches: k, ..Self::default() }
    }

    pub(super) fn json_members(&self) -> Vec<(String, JsonValue)> {
        let n = JsonValue::number;
        let mut m = vec![
            ("band".into(), s(self.band.key())),
            ("shape".into(), s(self.shape.key())),
            ("order".into(), n(self.order)),
        ];
        if let Some(v) = self.low {
            m.push(("low".into(), n(v)));
        }
        if let Some(v) = self.high {
            m.push(("high".into(), n(v)));
        }
        if !self.notches.is_empty() {
            m.push((
                "notches".into(),
                JsonValue::Array(
                    self.notches.iter().map(|p| JsonValue::Array(vec![n(p.fx), n(p.fy)])).collect(),
                ),
            ));
        }
        m.push(("auto_notches".into(), n(self.auto_notches as f64)));
        if let Some(r) = self.notch_radius {
            m.push(("notch_radius".into(), n(r)));
        }
        m.push(("notch_shape".into(), s(self.notch_shape.key())));
        m.push(("notch_pass".into(), JsonValue::Bool(self.notch_pass)));
        m.push(("padding".into(), s(self.padding.key())));
        m
    }

    pub(super) fn from_json(v: &JsonValue) -> Result<Self> {
        let d = Self::default();
        let band = match text(v, "band") {
            None => d.band,
            Some(k) => BandChoice::ALL.into_iter().find(|b| b.key() == k).ok_or_else(|| invalid(format!("unknown band {k:?}")))?,
        };
        let mut notches = Vec::new();
        if let Some(JsonValue::Array(list)) = v.get("notches") {
            for p in list {
                let pair = match p {
                    JsonValue::Array(xy) if xy.len() == 2 => xy[0].as_f64().zip(xy[1].as_f64()),
                    _ => None,
                };
                let (fx, fy) = pair.ok_or_else(|| invalid("each notch is a pair [fx, fy]"))?;
                notches.push(NotchPoint { fx, fy });
            }
        }
        Ok(Self {
            band,
            low: v.get("low").and_then(JsonValue::as_f64),
            high: v.get("high").and_then(JsonValue::as_f64),
            shape: ShapeChoice::from_key(text(v, "shape"), d.shape)?,
            order: number(v, "order", d.order),
            notches,
            auto_notches: count(v, "auto_notches", d.auto_notches),
            notch_radius: v.get("notch_radius").and_then(JsonValue::as_f64),
            notch_shape: ShapeChoice::from_key(text(v, "notch_shape"), d.notch_shape)?,
            notch_pass: flag(v, "notch_pass", d.notch_pass),
            padding: match text(v, "padding") {
                None => d.padding,
                k => PaddingChoice::from_key(k)?,
            },
        })
    }

    /// The band of the engine filter for an image with Nyquist `nyquist`.
    fn engine_band(&self, nyquist: f64) -> Result<Option<Band>> {
        let low = self.low.unwrap_or(0.25 * nyquist);
        let high = self.high.unwrap_or(0.5 * nyquist);
        let positive = |f: f64| f.is_finite() && f > 0.0;
        if self.band != BandChoice::None && !positive(low) {
            return Err(invalid("the cut-off frequency must be positive"));
        }
        if self.band.two_edges() && !(positive(high) && high > low) {
            return Err(invalid("the upper edge of the band must be above the lower edge"));
        }
        Ok(match self.band {
            BandChoice::None => None,
            BandChoice::Lowpass => Some(Band::Lowpass(low)),
            BandChoice::Highpass => Some(Band::Highpass(low)),
            BandChoice::Bandpass => Some(Band::Bandpass(low, high)),
            BandChoice::Bandreject => Some(Band::Bandstop(low, high)),
        })
    }

    /// One-line description of the filter for the summary.
    fn describe(&self, nyquist: f64, f: &impl Fn(f64) -> String) -> String {
        let low = self.low.unwrap_or(0.25 * nyquist);
        let high = self.high.unwrap_or(0.5 * nyquist);
        let shape = match self.shape {
            ShapeChoice::Butterworth => format!("Butterworth, order {}", format_number(self.order, 3)),
            other => other.label().to_owned(),
        };
        match self.band {
            BandChoice::None => "None".into(),
            BandChoice::Lowpass | BandChoice::Highpass => format!("{} at {} ({shape})", self.band.label(), f(low)),
            _ => format!("{} {} to {} ({shape})", self.band.label(), f(low), f(high)),
        }
    }
}

pub(super) fn run(
    table: &DataTable,
    states: &RowStateManager,
    input: &ImageInput,
    settings: &ImageFilterSettings,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    if settings.auto_notches > 20 {
        return Err(invalid("notch at most 20 of the strongest frequencies"));
    }
    if settings.band == BandChoice::None && settings.notches.is_empty() && settings.auto_notches == 0 {
        return Err(invalid("choose a band filter or at least one notch"));
    }
    let images = Images::extract(table, states, input, 4)?;
    let unit = images.frequency_unit();
    let fmt = |v: f64| format_number(v, 5);
    let first = &images.images[0].grid;
    let nyquist = (0.5 / first.dx).min(0.5 / first.dy);
    let mut summary = images.summary_rows();
    summary.push(("Band filter".into(), settings.describe(nyquist, &fmt)));
    summary.push(("Padding".into(), settings.padding.label().into()));
    let mut sections = vec![];
    let mut notch_table = TextTable::new(
        ["Image", "Notch", "Horizontal frequency", "Vertical frequency", "Period", "Source"]
            .into_iter()
            .skip(usize::from(images.group_name.is_none())),
    );
    let mut filtered_col = vec![f64::NAN; images.n_rows];
    let mut removed_col = vec![f64::NAN; images.n_rows];
    let mut image_sections = Vec::new();
    for (i, im) in images.images.iter().enumerate() {
        let g = &im.grid;
        let ny = (0.5 / g.dx).min(0.5 / g.dy);
        let band = settings.engine_band(ny)?;
        let step = (1.0 / (g.cols as f64 * g.dx)).max(1.0 / (g.rows as f64 * g.dy));
        let radius = settings.notch_radius.unwrap_or(2.0 * step);
        if !(radius.is_finite() && radius > 0.0) {
            return Err(invalid("the notch radius must be positive"));
        }
        let mut points: Vec<(NotchPoint, &'static str)> = settings.notches.iter().map(|&p| (p, "Given")).collect();
        let options = SpectrumOptions2 { remove_mean: true, taper: Taper2::Separable(Window::Hann), ..SpectrumOptions2::default() };
        let spec = spectrum2(g, &options)?;
        if settings.auto_notches > 0 {
            for p in dominant_frequencies(&spec, settings.auto_notches, 0.02 * ny) {
                points.push((NotchPoint { fx: p.fx, fy: p.fy }, "Strongest frequency"));
            }
        }
        let filter = FrequencyFilter {
            band: band.map(|b| (b, settings.shape.shape(settings.order))),
            notches: points.iter().map(|(p, _)| Notch { fx: p.fx, fy: p.fy, radius }).collect(),
            notch_shape: settings.notch_shape.shape(settings.order),
            notch_pass: settings.notch_pass,
            padding: settings.padding.padding(g.rows, g.cols),
        };
        let out = frequency_filter(g, &filter)?;
        im.fill_rows(&out.filtered, &mut filtered_col);
        im.fill_rows(&out.removed, &mut removed_col);
        let label = im.label.clone();
        for (k, (p, source)) in points.iter().enumerate() {
            let rho = p.fx.hypot(p.fy);
            let mut row = vec![
                (k + 1).to_string(),
                fmt(p.fx),
                fmt(p.fy),
                if rho > 0.0 { fmt(1.0 / rho) } else { "—".into() },
                (*source).to_owned(),
            ];
            if images.group_name.is_some() {
                row.insert(0, label.clone());
            }
            notch_table.push(row);
        }
        if i < images.drawn() {
            let suffix = images.suffix(i);
            image_sections.push(images.image_section(i, SectionKind::ImagePlot, "Original Image", g, &images.name));
            image_sections.push(images.image_section(
                i,
                SectionKind::ImageFiltered,
                "Filtered Image",
                &out.filtered,
                &format!("Filtered {}", images.name),
            ));
            image_sections.push(images.image_section(
                i,
                SectionKind::ImageRemoved,
                "Removed by the Filter",
                &out.removed,
                "Original − filtered",
            ));
            let mut db = spec.power.clone();
            db.data = decibels(&spec.power.data);
            let marks = points
                .iter()
                .enumerate()
                .flat_map(|(k, (p, _))| {
                    [1.0, -1.0].map(|sgn| RasterMark { at: [sgn * p.fx, sgn * p.fy], label: (k + 1).to_string(), shape: MarkShape::Cross })
                })
                .collect();
            image_sections.push(frequency_section(
                SectionKind::ImageSpectrumMagnitude,
                &format!("Spectrum with Notches{suffix}"),
                &db,
                &unit,
                "Power (dB)",
                marks,
            ));
            image_sections.push(frequency_section(
                SectionKind::ImageFilterMask,
                &format!("Filter Mask{suffix}"),
                &out.mask,
                &unit,
                "Gain",
                Vec::new(),
            ));
        }
    }
    if !notch_table.rows.is_empty() {
        let radius_text = settings.notch_radius.map_or_else(|| "two frequency steps".to_owned(), fmt);
        summary.push(("Notches".into(), format!(
            "{} {} ({}, radius {radius_text})",
            notch_table.rows.len(),
            if settings.notch_pass { "kept (notch-pass)" } else { "removed" },
            settings.notch_shape.label()
        )));
    }
    sections.push(section(SectionKind::ImageSummary, "Summary", SectionContent::Table(summary_table(summary))));
    sections.extend(image_sections);
    if !notch_table.rows.is_empty() {
        sections.push(section(SectionKind::ImageNotches, "Notches", SectionContent::Table(notch_table)));
    }
    let name = &images.name;
    let columns = vec![
        ReportColumn::continuous("filtered", "Save filtered image", format!("Filtered {name}"), filtered_col),
        ReportColumn::continuous("removed", "Save removed part", format!("Removed from {name}"), removed_col),
    ];
    Ok((format!("Image Frequency Filter: {name}"), sections, columns))
}

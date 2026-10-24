//! Reports as PowerPoint presentations (Office Open XML PresentationML,
//! ECMA-376), 16:9 slides written as XML parts in a zip container.
//!
//! The first slide carries the title and the line naming the table, its
//! rows and the export date. When several reports are exported each starts
//! with a slide of its title. Every open section then gets slides of its
//! own, titled with the section title (a real title placeholder, so the
//! outline view and screen readers find it):
//!
//! * a plot as a picture fitted to the slide (a 300 ppi PNG with the SVG
//!   alongside, which PowerPoint 2016 and later draw instead);
//! * a table as a native table, set smaller as it gets wider, its rows
//!   spread over as many slides as they need (titled "… (continued)") and
//!   very wide tables split into column groups that each repeat the first
//!   column; numeric columns are right-aligned;
//! * notes and formulas as text, spread over slides the same way.

use std::fmt::Write as _;
use std::io::Write as _;

use stat_viz::export::VectorFormat;

use super::docx::xml;
use super::{Block, DocTable, ReportDocument, png_size, svg_size};

/// English Metric Units per point.
const EMU: f64 = 12_700.0;
/// Slide size in points (13.333 × 7.5 in).
const SLIDE_W: f64 = 960.0;
const SLIDE_H: f64 = 540.0;
/// Content area below the title.
const LEFT: f64 = 36.0;
const TOP: f64 = 96.0;
const WIDTH: f64 = SLIDE_W - 2.0 * LEFT;
const HEIGHT: f64 = SLIDE_H - TOP - 30.0;

const INK: &str = "111827";
const MUTED: &str = "5A6270";
const RULE: &str = "374151";
const ZEBRA: &str = "F3F4F6";

fn emu(pt: f64) -> i64 {
    (pt * EMU).round() as i64
}

/// One slide's shapes and pictures.
#[derive(Default)]
struct Slide {
    title: String,
    /// A title-only slide (the document or a report), set large and centred.
    divider: Option<String>,
    shapes: String,
    pictures: Vec<(Vec<u8>, Vec<u8>)>,
    next_id: u32,
}

impl Slide {
    fn new(title: &str) -> Self {
        Self { title: title.to_owned(), next_id: 3, ..Self::default() }
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }
}

fn text_run(text: &str, size_pt: f64, bold: bool, color: &str, mono: bool) -> String {
    let b = if bold { " b=\"1\"" } else { "" };
    let font = if mono { "<a:latin typeface=\"Consolas\"/><a:cs typeface=\"Consolas\"/>" } else { "" };
    format!(
        "<a:r><a:rPr lang=\"en-US\" sz=\"{}\"{b} dirty=\"0\"><a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill>{font}</a:rPr><a:t>{}</a:t></a:r>",
        (size_pt * 100.0).round() as i64,
        xml(text)
    )
}

/// A text box at `(x, y, w, h)` points holding `paragraphs` (each already
/// a run list).
fn text_box(id: u32, name: &str, rect: [f64; 4], paragraphs: &[String], anchor: &str, centred: bool) -> String {
    let algn = if centred { "<a:pPr algn=\"ctr\"/>" } else { "" };
    let body: String = paragraphs.iter().map(|p| format!("<a:p>{algn}{p}</a:p>")).collect();
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr>\
         <p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>\
         <p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"{anchor}\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{body}</p:txBody></p:sp>",
        emu(rect[0]),
        emu(rect[1]),
        emu(rect[2]),
        emu(rect[3])
    )
}

/// Rough width in points of `text` at `size` (average glyph ~0.52 em), for
/// column widths and line counts.
fn approx_width(text: &str, size: f64) -> f64 {
    text.chars().count() as f64 * size * 0.58
}

/// Lines `text` takes when wrapped to `width` points at `size`.
fn wrapped_lines(text: &str, size: f64, width: f64) -> Vec<String> {
    let per_line = ((width / (size * 0.52)).floor() as usize).max(8);
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > per_line {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
            while line.chars().count() > per_line {
                let head: String = line.chars().take(per_line).collect();
                line = line.chars().skip(per_line).collect();
                out.push(head);
            }
        }
        out.push(line);
    }
    out
}

/// A table's font size: the largest from 18 pt down to 8 pt at which it
/// fits the slide's width and, when it can, all its rows fit one slide.
fn table_size(t: &DocTable, cols: &[usize]) -> f64 {
    let natural = |size: f64| -> f64 {
        cols.iter()
            .map(|&j| {
                t.table
                    .rows
                    .iter()
                    .filter_map(|r| r.get(j))
                    .chain(std::iter::once(&t.table.columns[j]))
                    .map(|c| approx_width(c, size))
                    .fold(0.0, f64::max)
                    + 12.0
            })
            .sum()
    };
    const SIZES: [f64; 9] = [18.0, 16.0, 14.0, 13.0, 12.0, 11.0, 10.0, 9.0, 8.0];
    let rows = (t.table.rows.len() + 1) as f64;
    SIZES
        .into_iter()
        .find(|&s| natural(s) <= WIDTH && rows * s * 1.75 <= HEIGHT)
        .or_else(|| SIZES.into_iter().find(|&s| natural(s) <= WIDTH && s <= 12.0))
        .unwrap_or(8.0)
}

/// Column groups of `t` that each fit the slide at 8 pt, each led by the
/// first column.
fn column_groups(t: &DocTable) -> Vec<Vec<usize>> {
    let n = t.table.columns.len();
    let all: Vec<usize> = (0..n).collect();
    let width = |cols: &[usize]| -> f64 {
        cols.iter()
            .map(|&j| {
                t.table
                    .rows
                    .iter()
                    .filter_map(|r| r.get(j))
                    .chain(std::iter::once(&t.table.columns[j]))
                    .map(|c| approx_width(c, 8.0).min(WIDTH * 0.4))
                    .fold(0.0, f64::max)
                    + 12.0
            })
            .sum()
    };
    if n <= 1 || width(&all) <= WIDTH {
        return vec![all];
    }
    let mut groups = Vec::new();
    let mut next = 1;
    while next < n {
        let mut cols = vec![0, next];
        next += 1;
        while next < n {
            let mut more = cols.clone();
            more.push(next);
            if width(&more) > WIDTH {
                break;
            }
            cols = more;
            next += 1;
        }
        groups.push(cols);
    }
    groups
}

fn table_frame(id: u32, t: &DocTable, cols: &[usize], rows: &[usize], size: f64) -> (String, f64) {
    let pad = 6.0;
    let mut widths: Vec<f64> = cols
        .iter()
        .map(|&j| {
            t.table
                .rows
                .iter()
                .filter_map(|r| r.get(j))
                .chain(std::iter::once(&t.table.columns[j]))
                .map(|c| approx_width(c, size).min(WIDTH * 0.4))
                .fold(approx_width(&t.table.columns[j], size) * 1.08, f64::max)
                + 2.0 * pad
        })
        .collect();
    let total: f64 = widths.iter().sum();
    if total > WIDTH {
        widths.iter_mut().for_each(|w| *w *= WIDTH / total);
    }
    let row_h = size * 1.75;
    let line = |side: &str, w: f64| {
        format!("<a:{side} w=\"{}\"><a:solidFill><a:srgbClr val=\"{RULE}\"/></a:solidFill></a:{side}>", emu(w))
    };
    let mut out = format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>\
         <p:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></p:xfrm>\
         <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr firstRow=\"1\"/><a:tblGrid>",
        emu(LEFT + (WIDTH - widths.iter().sum::<f64>()).max(0.0) / 2.0),
        emu(TOP),
        emu(widths.iter().sum()),
        emu(row_h * (rows.len() + 1) as f64)
    );
    for w in &widths {
        let _ = write!(out, "<a:gridCol w=\"{}\"/>", emu(*w));
    }
    out.push_str("</a:tblGrid>");
    let cell = |out: &mut String, text: &str, right: bool, header: bool, last: bool, zebra: bool| {
        let algn = if right { "<a:pPr algn=\"r\"/>" } else { "" };
        let para = if text.is_empty() {
            format!("<a:p>{algn}<a:endParaRPr lang=\"en-US\" sz=\"{}\" dirty=\"0\"/></a:p>", (size * 100.0) as i64)
        } else {
            format!("<a:p>{algn}{}</a:p>", text_run(text, size, header, INK, false))
        };
        let mut lines = String::from("<a:lnL w=\"0\"><a:noFill/></a:lnL><a:lnR w=\"0\"><a:noFill/></a:lnR>");
        lines.push_str(&if header { line("lnT", 1.0) } else { "<a:lnT w=\"0\"><a:noFill/></a:lnT>".to_owned() });
        lines.push_str(&if header {
            line("lnB", 0.5)
        } else if last {
            line("lnB", 1.0)
        } else {
            "<a:lnB w=\"0\"><a:noFill/></a:lnB>".to_owned()
        });
        let fill = if zebra {
            format!("<a:solidFill><a:srgbClr val=\"{ZEBRA}\"/></a:solidFill>")
        } else {
            "<a:noFill/>".to_owned()
        };
        let _ = write!(
            out,
            "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{para}</a:txBody><a:tcPr marL=\"{m}\" marR=\"{m}\" marT=\"{v}\" marB=\"{v}\" anchor=\"ctr\">{lines}{fill}</a:tcPr></a:tc>",
            m = emu(pad),
            v = emu(size * 0.2)
        );
    };
    let _ = write!(out, "<a:tr h=\"{}\">", emu(row_h));
    for &j in cols {
        cell(&mut out, &t.table.columns[j], t.numeric[j], true, false, false);
    }
    out.push_str("</a:tr>");
    for (k, &r) in rows.iter().enumerate() {
        let _ = write!(out, "<a:tr h=\"{}\">", emu(row_h));
        for &j in cols {
            let text = t.table.rows[r].get(j).map_or("", String::as_str);
            cell(&mut out, text, t.right_aligned(j, text), false, k + 1 == rows.len(), k % 2 == 1);
        }
        out.push_str("</a:tr>");
    }
    out.push_str("</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
    (out, row_h)
}

fn picture(slide: &mut Slide, png: Vec<u8>, svg: Vec<u8>, alt: &str) {
    let (w, h) = svg_size(&svg)
        .or_else(|| png_size(&png).map(|(w, h)| (f64::from(w) * 0.24, f64::from(h) * 0.24)))
        .unwrap_or((540.0, 380.0));
    let scale = (WIDTH / w).min(HEIGHT / h);
    let (pw, ph) = (w * scale, h * scale);
    let (x, y) = (LEFT + (WIDTH - pw) / 2.0, TOP + (HEIGHT - ph) / 2.0);
    let k = slide.pictures.len();
    let id = slide.id();
    let _ = write!(
        slide.shapes,
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"Plot {id}\" descr=\"{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
         <p:blipFill><a:blip r:embed=\"rIdPng{k}\"><a:extLst><a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\">\
         <asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"rIdSvg{k}\"/></a:ext></a:extLst></a:blip>\
         <a:stretch><a:fillRect/></a:stretch></p:blipFill>\
         <p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        xml(alt),
        emu(x),
        emu(y),
        emu(pw),
        emu(ph)
    );
    slide.pictures.push((png, svg));
}

/// Slides for lines of text, as many as they need.
fn text_slides(slides: &mut Vec<Slide>, title: &str, text: &str, mono: bool) {
    let size = if mono { 12.0 } else { 16.0 };
    let lines = wrapped_lines(text, size, WIDTH);
    let per_slide = ((HEIGHT / (size * 1.3)).floor() as usize).max(1);
    for (k, chunk) in lines.chunks(per_slide).enumerate() {
        let mut slide = Slide::new(&continued(title, k));
        let paragraphs: Vec<String> =
            chunk.iter().map(|l| text_run(l, size, false, if mono { INK } else { MUTED }, mono)).collect();
        let id = slide.id();
        slide.shapes.push_str(&text_box(id, "Text", [LEFT, TOP, WIDTH, HEIGHT], &paragraphs, "t", false));
        slides.push(slide);
    }
}

fn continued(title: &str, k: usize) -> String {
    if k == 0 { title.to_owned() } else { format!("{title} (continued)") }
}

fn slides(doc: &ReportDocument<'_>) -> Vec<Slide> {
    let mut out = Vec::new();
    let mut cover = Slide::new(&doc.title);
    cover.divider = Some(doc.meta.clone());
    out.push(cover);
    let deepest = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading(h) => Some(h.level),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    let mut section = doc.title.clone();
    for b in &doc.blocks {
        match b {
            Block::Heading(h) if h.level < deepest => {
                let mut s = Slide::new(&h.text);
                s.divider = Some(String::new());
                out.push(s);
            }
            Block::Heading(h) => section.clone_from(&h.text),
            Block::Note(n) => text_slides(&mut out, &section, n, false),
            Block::Formula(f) => text_slides(&mut out, &section, f, true),
            Block::Table(t) if t.table.columns.is_empty() => {}
            Block::Table(t) => {
                let mut k = 0;
                for cols in column_groups(t) {
                    let size = table_size(t, &cols);
                    let row_h = size * 1.75;
                    let per_slide = ((HEIGHT / row_h).floor() as usize).saturating_sub(1).max(1);
                    let rows: Vec<usize> = (0..t.table.rows.len()).collect();
                    let chunks: Vec<&[usize]> =
                        if rows.is_empty() { vec![&[]] } else { rows.chunks(per_slide).collect() };
                    for chunk in chunks {
                        let mut slide = Slide::new(&continued(&section, k));
                        let id = slide.id();
                        slide.shapes.push_str(&table_frame(id, t, &cols, chunk, size).0);
                        out.push(slide);
                        k += 1;
                    }
                }
            }
            Block::Figure(f) => {
                let mut slide = Slide::new(&section);
                match f.render(VectorFormat::Svg).and_then(|svg| Ok((f.render(VectorFormat::Png)?, svg))) {
                    Ok((png, svg)) => picture(&mut slide, png, svg, &f.alt_text()),
                    Err(e) => {
                        let id = slide.id();
                        let p = [text_run(&e, 16.0, false, MUTED, false)];
                        slide.shapes.push_str(&text_box(id, "Text", [LEFT, TOP, WIDTH, HEIGHT], &p, "t", false));
                    }
                }
                out.push(slide);
            }
        }
    }
    out
}

const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
     xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
     xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";

const GROUP: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
     <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";

/// The title placeholder: at the top of content slides, centred on title
/// slides.
fn title_shape(text: &str, divider: bool) -> String {
    let (rect, size, anchor) =
        if divider { ([LEFT, 150.0, WIDTH, 140.0], 36.0, "b") } else { ([LEFT, 24.0, WIDTH, 56.0], 26.0, "b") };
    let run = if text.is_empty() { String::new() } else { text_run(text, size, true, INK, false) };
    let algn = if divider { "<a:pPr algn=\"ctr\"/>" } else { "" };
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
         <p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm></p:spPr>\
         <p:txBody><a:bodyPr anchor=\"{anchor}\"><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p>{algn}{run}</a:p></p:txBody></p:sp>",
        emu(rect[0]),
        emu(rect[1]),
        emu(rect[2]),
        emu(rect[3])
    )
}

fn slide_xml(slide: &Slide) -> String {
    let mut tree = String::from(GROUP);
    tree.push_str(&title_shape(&slide.title, slide.divider.is_some()));
    if let Some(sub) = slide.divider.as_deref().filter(|s| !s.is_empty()) {
        tree.push_str(&text_box(
            3,
            "Subtitle",
            [LEFT, 300.0, WIDTH, 40.0],
            &[text_run(sub, 16.0, false, MUTED, false)],
            "t",
            true,
        ));
    }
    tree.push_str(&slide.shapes);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sld {NS}><p:cSld><p:spTree>{tree}</p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>\n"
    )
}

const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="AnovaCraft"><a:themeElements>
<a:clrScheme name="AnovaCraft"><a:dk1><a:srgbClr val="111827"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="374151"/></a:dk2><a:lt2><a:srgbClr val="F3F4F6"/></a:lt2>
<a:accent1><a:srgbClr val="2563EB"/></a:accent1><a:accent2><a:srgbClr val="E11D48"/></a:accent2><a:accent3><a:srgbClr val="059669"/></a:accent3><a:accent4><a:srgbClr val="D97706"/></a:accent4><a:accent5><a:srgbClr val="7C3AED"/></a:accent5><a:accent6><a:srgbClr val="0891B2"/></a:accent6>
<a:hlink><a:srgbClr val="2563EB"/></a:hlink><a:folHlink><a:srgbClr val="7C3AED"/></a:folHlink></a:clrScheme>
<a:fontScheme name="AnovaCraft"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>
<a:fmtScheme name="AnovaCraft"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
<a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme>
</a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>
"#;

fn parts(doc: &ReportDocument<'_>) -> Vec<(String, Vec<u8>)> {
    let slides = slides(doc);
    let title_ph = title_shape("", false);
    let master = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sldMaster {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>\
         <p:spTree>{GROUP}{title_ph}</p:spTree></p:cSld>\
         <p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>\
         <p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>\
         <p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr sz=\"2600\" b=\"1\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mj-lt\"/></a:defRPr></a:lvl1pPr></p:titleStyle>\
         <p:bodyStyle><a:lvl1pPr><a:defRPr sz=\"1600\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl1pPr></p:bodyStyle>\
         <p:otherStyle><a:lvl1pPr><a:defRPr sz=\"1400\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>\n"
    );
    let layout = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sldLayout {NS} type=\"titleOnly\" preserve=\"1\"><p:cSld name=\"Title Only\"><p:spTree>{GROUP}{title_ph}</p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>\n"
    );
    let rel = |id: &str, kind: &str, target: &str| {
        format!(
            "<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{target}\"/>"
        )
    };
    let rels = |body: String| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{body}</Relationships>\n"
        )
    };
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut ids = String::new();
    let mut pres_rels = rel("rIdMaster", "slideMaster", "slideMasters/slideMaster1.xml");
    pres_rels.push_str(&rel("rIdTheme", "theme", "theme/theme1.xml"));
    let mut overrides = String::new();
    for (i, slide) in slides.iter().enumerate() {
        let n = i + 1;
        let _ = write!(ids, "<p:sldId id=\"{}\" r:id=\"rIdSlide{n}\"/>", 255 + n);
        pres_rels.push_str(&rel(&format!("rIdSlide{n}"), "slide", &format!("slides/slide{n}.xml")));
        let _ = write!(
            overrides,
            "<Override PartName=\"/ppt/slides/slide{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        );
        let mut slide_rels = rel("rIdLayout", "slideLayout", "../slideLayouts/slideLayout1.xml");
        for (k, (png, svg)) in slide.pictures.iter().enumerate() {
            slide_rels.push_str(&rel(&format!("rIdPng{k}"), "image", &format!("../media/slide{n}-plot{k}.png")));
            slide_rels.push_str(&rel(&format!("rIdSvg{k}"), "image", &format!("../media/slide{n}-plot{k}.svg")));
            out.push((format!("ppt/media/slide{n}-plot{k}.png"), png.clone()));
            out.push((format!("ppt/media/slide{n}-plot{k}.svg"), svg.clone()));
        }
        out.push((format!("ppt/slides/slide{n}.xml"), slide_xml(slide).into_bytes()));
        out.push((format!("ppt/slides/_rels/slide{n}.xml.rels"), rels(slide_rels).into_bytes()));
    }
    let presentation = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:presentation {NS} saveSubsetFonts=\"1\">\
         <p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rIdMaster\"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst>\
         <p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>\n",
        emu(SLIDE_W),
        emu(SLIDE_H)
    );
    let content_types = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>\
         <Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
         <Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
         <Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\
         <Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
         <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
         <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
         {overrides}</Types>\n"
    );
    let package_rels = rels(
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
             <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>"
            .to_owned(),
    );
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
         <dc:title>{}</dc:title><dc:creator>AnovaCraft</dc:creator></cp:coreProperties>\n",
        xml(&doc.title)
    );
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>AnovaCraft</Application></Properties>\n";
    let mut parts = vec![
        ("[Content_Types].xml".to_owned(), content_types.into_bytes()),
        ("_rels/.rels".to_owned(), package_rels.into_bytes()),
        ("docProps/core.xml".to_owned(), core.into_bytes()),
        ("docProps/app.xml".to_owned(), app.as_bytes().to_vec()),
        ("ppt/presentation.xml".to_owned(), presentation.into_bytes()),
        ("ppt/_rels/presentation.xml.rels".to_owned(), rels(pres_rels).into_bytes()),
        ("ppt/slideMasters/slideMaster1.xml".to_owned(), master.into_bytes()),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_owned(),
            rels(format!(
                "{}{}",
                rel("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"),
                rel("rId2", "theme", "../theme/theme1.xml")
            ))
            .into_bytes(),
        ),
        ("ppt/slideLayouts/slideLayout1.xml".to_owned(), layout.into_bytes()),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_owned(),
            rels(rel("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")).into_bytes(),
        ),
        ("ppt/theme/theme1.xml".to_owned(), THEME.as_bytes().to_vec()),
    ];
    parts.extend(out);
    parts
}

/// `doc` as a `.pptx` file.
///
/// # Errors
/// When the zip container cannot be written.
pub fn write(doc: &ReportDocument<'_>) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let fail = |e: &dyn std::fmt::Display| format!("could not write the presentation: {e}");
    for (name, bytes) in parts(doc) {
        let method = if name.ends_with(".png") { stored } else { deflated };
        zip.start_file(name, method).map_err(|e| fail(&e))?;
        zip.write_all(&bytes).map_err(|e| fail(&e))?;
    }
    Ok(zip.finish().map_err(|e| fail(&e))?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_export::HtmlReport;
    use crate::report::TextTable;
    use crate::report_export::ExportScope;
    use crate::report_export::tests::{plant_growth, source};

    #[test]
    fn parts_are_well_formed_and_linked() {
        let (table, rows, computed) = plant_growth();
        let all: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
        let doc = ReportDocument::new(ExportScope::All(&all), source(&table, &rows));
        let parts = parts(&doc);
        let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
        for (name, bytes) in &parts {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let text = std::str::from_utf8(bytes).unwrap();
                roxmltree::Document::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
            // Every relationship of a part points at a part in the package.
            if let Some(rest) = name.strip_suffix(".rels") {
                let dir = rest.rsplit_once("/_rels/").map_or("", |(d, _)| d);
                for target in
                    std::str::from_utf8(bytes).unwrap().split("Target=\"").skip(1).map(|s| &s[..s.find('"').unwrap()])
                {
                    let mut path: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
                    for piece in target.split('/') {
                        if piece == ".." {
                            path.pop();
                        } else {
                            path.push(piece);
                        }
                    }
                    let full = path.join("/");
                    assert!(names.contains(&full.as_str()), "{name} -> {full}");
                }
            }
        }
        let text: String = parts
            .iter()
            .filter(|(n, _)| n.starts_with("ppt/slides/slide"))
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .collect();
        for r in &computed {
            assert!(text.contains(&xml(&r.title)));
            for s in &r.sections {
                assert!(text.contains(&xml(&s.title)), "{}", s.title);
            }
        }
        assert!(write(&doc).unwrap().starts_with(b"PK"));
    }

    #[test]
    fn long_and_wide_tables_spread_over_slides() {
        let mut t = TextTable::new(["Row", "Value"]);
        for i in 0..100 {
            t.push([format!("{i}"), format!("{i}.5")]);
        }
        let long = DocTable::new(t);
        let doc = ReportDocument {
            title: "T".into(),
            meta: String::new(),
            blocks: vec![Block::Heading(super::super::Heading { level: 1, text: "Values".into() }), Block::Table(long)],
            reports: vec![],
        };
        let s = slides(&doc);
        assert!(s.len() > 3);
        assert_eq!(s[1].title, "Values");
        assert_eq!(s[2].title, "Values (continued)");
        let names: Vec<String> = (0..40).map(|i| format!("Variable {i}")).collect();
        let mut t = TextTable::new(names.clone());
        t.push(names.iter().map(|_| "-0.123456".to_owned()));
        let groups = column_groups(&DocTable::new(t));
        assert!(groups.len() > 1 && groups.iter().all(|g| g[0] == 0));
        assert_eq!(groups.iter().map(|g| g.len() - 1).sum::<usize>(), 39);
    }
}

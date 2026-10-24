//! Reports as Word documents (Office Open XML WordprocessingML,
//! ECMA-376), written as XML parts in a zip container.
//!
//! The document uses Word's built-in style names (Title, Heading 1–3, so the
//! navigation pane and a table of contents pick the headings up) plus a few
//! of its own: "Report Meta" for the line under the title, "Report Note",
//! "Report Formula" (monospace, shaded) and the table style "Report Table"
//! (rules above and below and under the header row). Tables mark their
//! header row to repeat on every page, numeric columns are right-aligned,
//! and plots are inline pictures: a 300 ppi PNG, with the SVG alongside
//! through the `asvg:svgBlip` extension, which Word 2016 and later draw
//! instead of the PNG. The page size is Letter or A4 with 0.75 in margins,
//! and the footer shows "Page n of N".

use std::fmt::Write as _;
use std::io::Write as _;

use stat_viz::export::VectorFormat;

use super::{Block, DocTable, ExportOptions, ReportDocument, png_size, svg_size};

/// English Metric Units per point.
const EMU_PER_PT: f64 = 12_700.0;
/// Margin in twentieths of a point (0.75 in).
const MARGIN_TWIPS: u32 = 1080;

/// `text` escaped for XML text and attribute values; characters XML 1.0
/// does not allow are dropped.
pub(super) fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// A run of text, keeping spaces.
fn run(text: &str, props: &str) -> String {
    let props = if props.is_empty() { String::new() } else { format!("<w:rPr>{props}</w:rPr>") };
    format!("<w:r>{props}<w:t xml:space=\"preserve\">{}</w:t></w:r>", xml(text))
}

/// Runs for multi-line text: lines joined by line breaks.
fn runs(text: &str, props: &str) -> String {
    let props_xml = if props.is_empty() { String::new() } else { format!("<w:rPr>{props}</w:rPr>") };
    text.split('\n')
        .enumerate()
        .map(|(i, line)| {
            let br = if i > 0 { "<w:br/>" } else { "" };
            format!("<w:r>{props_xml}{br}<w:t xml:space=\"preserve\">{}</w:t></w:r>", xml(line))
        })
        .collect()
}

fn paragraph(style: &str, body: &str) -> String {
    format!("<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>{body}</w:p>")
}

/// A picture to place in the document.
struct Picture {
    png: Vec<u8>,
    svg: Vec<u8>,
    width_pt: f64,
    height_pt: f64,
    alt: String,
}

fn drawing(index: usize, png_rel: &str, svg_rel: &str, p: &Picture, text_width_pt: f64) -> String {
    let scale = (text_width_pt / p.width_pt).min(1.0);
    let cx = (p.width_pt * scale * EMU_PER_PT).round() as u64;
    let cy = (p.height_pt * scale * EMU_PER_PT).round() as u64;
    let id = index + 1;
    let alt = xml(&p.alt);
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"ReportFigure\"/></w:pPr><w:r><w:drawing>\
         <wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
         <wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Plot {id}\" descr=\"{alt}\"/>\
         <wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
         <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
         <pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"plot{id}.png\" descr=\"{alt}\"/><pic:cNvPicPr/></pic:nvPicPr>\
         <pic:blipFill><a:blip r:embed=\"{png_rel}\"><a:extLst>\
         <a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\"><asvg:svgBlip r:embed=\"{svg_rel}\"/></a:ext>\
         </a:extLst></a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
         <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
         </a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    )
}

/// Rough width of a cell's text in points at 9 pt, for the table grid
/// (Word re-fits the columns when it opens the file).
fn text_points(text: &str) -> f64 {
    text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * 4.9 + 12.0
}

fn table(t: &DocTable, text_width_pt: f64) -> String {
    let n = t.table.columns.len();
    let mut widths: Vec<f64> = (0..n)
        .map(|j| {
            t.table
                .rows
                .iter()
                .filter_map(|r| r.get(j))
                .chain(std::iter::once(&t.table.columns[j]))
                .map(|c| text_points(c))
                .fold(0.0, f64::max)
        })
        .collect();
    let total: f64 = widths.iter().sum();
    if total > text_width_pt {
        for w in &mut widths {
            *w *= text_width_pt / total;
        }
    }
    let twips = |pt: f64| (pt * 20.0).round() as u64;
    let mut out = String::from(
        "<w:tbl><w:tblPr><w:tblStyle w:val=\"ReportTable\"/><w:tblW w:w=\"0\" w:type=\"auto\"/>\
         <w:tblLook w:val=\"0420\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" \
         w:noHBand=\"1\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>",
    );
    for w in &widths {
        let _ = write!(out, "<w:gridCol w:w=\"{}\"/>", twips(*w));
    }
    out.push_str("</w:tblGrid>");
    let cell = |out: &mut String, text: &str, right: bool, header: bool, w: f64| {
        let jc = if right { "<w:jc w:val=\"right\"/>" } else { "" };
        let border = if header {
            "<w:tcBorders><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"374151\"/></w:tcBorders>"
        } else {
            ""
        };
        let props = if header { "<w:b/>" } else { "" };
        let _ = write!(
            out,
            "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>{border}</w:tcPr><w:p><w:pPr><w:pStyle w:val=\"ReportTableText\"/>{jc}</w:pPr>{}</w:p></w:tc>",
            twips(w),
            runs(text, props)
        );
    };
    out.push_str("<w:tr><w:trPr><w:tblHeader/><w:cantSplit/></w:trPr>");
    for (j, h) in t.table.columns.iter().enumerate() {
        cell(&mut out, h, t.numeric[j], true, widths[j]);
    }
    out.push_str("</w:tr>");
    for row in &t.table.rows {
        out.push_str("<w:tr><w:trPr><w:cantSplit/></w:trPr>");
        for j in 0..n {
            let text = row.get(j).map_or("", String::as_str);
            cell(&mut out, text, t.right_aligned(j, text), false, widths[j]);
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    // Word needs a paragraph between consecutive tables; this one also
    // spaces the table from what follows.
    out.push_str("<w:p><w:pPr><w:pStyle w:val=\"ReportSpacer\"/></w:pPr></w:p>");
    out
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="20"/><w:szCs w:val="20"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="80" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:after="40"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:color w:val="111827"/><w:sz w:val="36"/><w:szCs w:val="36"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="360" w:after="120"/><w:pBdr><w:bottom w:val="single" w:sz="4" w:space="2" w:color="D1D5DB"/></w:pBdr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:color w:val="111827"/><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:color w:val="111827"/><w:sz w:val="23"/><w:szCs w:val="23"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ReportMeta"><w:name w:val="Report Meta"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:spacing w:after="240"/><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="4" w:color="374151"/></w:pBdr></w:pPr><w:rPr><w:color w:val="5A6270"/><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ReportNote"><w:name w:val="Report Note"/><w:basedOn w:val="Normal"/><w:rPr><w:color w:val="5A6270"/><w:sz w:val="19"/><w:szCs w:val="19"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ReportFormula"><w:name w:val="Report Formula"/><w:basedOn w:val="Normal"/><w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F3F4F6"/><w:spacing w:after="160" w:line="240" w:lineRule="auto"/><w:ind w:left="120" w:right="120"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="17"/><w:szCs w:val="17"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ReportFigure"><w:name w:val="Report Figure"/><w:basedOn w:val="Normal"/><w:pPr><w:jc w:val="center"/><w:spacing w:after="200"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="ReportTableText"><w:name w:val="Report Table Text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="20" w:after="20" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="17"/><w:szCs w:val="17"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ReportSpacer"><w:name w:val="Report Spacer"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="120" w:line="120" w:lineRule="exact"/></w:pPr><w:rPr><w:sz w:val="8"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Footer"><w:name w:val="footer"/><w:basedOn w:val="Normal"/><w:pPr><w:tabs><w:tab w:val="right" w:pos="TEXT_WIDTH"/></w:tabs><w:spacing w:after="0"/></w:pPr><w:rPr><w:color w:val="5A6270"/><w:sz w:val="16"/><w:szCs w:val="16"/></w:rPr></w:style>
<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
<w:style w:type="table" w:styleId="ReportTable"><w:name w:val="Report Table"/><w:basedOn w:val="TableNormal"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="8" w:space="0" w:color="374151"/><w:bottom w:val="single" w:sz="8" w:space="0" w:color="374151"/></w:tblBorders><w:tblCellMar><w:left w:w="80" w:type="dxa"/><w:right w:w="80" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
</w:styles>
"#;

const NAMESPACES: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
     xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
     xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
     xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
     xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" \
     xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\"";

/// The parts of a document: `(path in the zip, contents)`.
fn parts(doc: &ReportDocument<'_>, options: ExportOptions) -> Vec<(String, Vec<u8>)> {
    let (pw, ph) = options.paper.points();
    let text_width_pt = pw - 2.0 * f64::from(MARGIN_TWIPS) / 20.0;
    let mut body = String::new();
    body.push_str(&paragraph("Title", &run(&doc.title, "")));
    body.push_str(&paragraph("ReportMeta", &run(&doc.meta, "")));
    let deepest = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading(h) => Some(h.level),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    let mut pictures: Vec<Picture> = Vec::new();
    for b in &doc.blocks {
        match b {
            Block::Heading(h) => {
                let style = if h.level < deepest {
                    "Heading1"
                } else if deepest > 1 {
                    "Heading2"
                } else {
                    "Heading1"
                };
                body.push_str(&paragraph(style, &run(&h.text, "")));
            }
            Block::Note(n) => body.push_str(&paragraph("ReportNote", &runs(n, ""))),
            Block::Formula(f) => body.push_str(&paragraph("ReportFormula", &runs(f, ""))),
            Block::Table(t) if !t.table.columns.is_empty() => body.push_str(&table(t, text_width_pt)),
            Block::Table(_) => {}
            Block::Figure(f) => {
                let figure = f.render(VectorFormat::Svg).and_then(|svg| Ok((svg, f.render(VectorFormat::Png)?)));
                match figure {
                    Ok((svg, png)) => {
                        let (width_pt, height_pt) = svg_size(&svg)
                            .or_else(|| png_size(&png).map(|(w, h)| (f64::from(w) * 0.24, f64::from(h) * 0.24)))
                            .unwrap_or((540.0, 380.0));
                        let p = Picture { png, svg, width_pt, height_pt, alt: f.alt_text() };
                        let i = pictures.len();
                        body.push_str(&drawing(i, &format!("rIdPng{i}"), &format!("rIdSvg{i}"), &p, text_width_pt));
                        pictures.push(p);
                    }
                    Err(e) => body.push_str(&paragraph("ReportNote", &runs(&e, ""))),
                }
            }
        }
    }
    let twips = |pt: f64| (pt * 20.0).round() as u64;
    let _ = write!(
        body,
        "<w:sectPr><w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/>\
         <w:pgSz w:w=\"{}\" w:h=\"{}\"/>\
         <w:pgMar w:top=\"{m}\" w:right=\"{m}\" w:bottom=\"{m}\" w:left=\"{m}\" w:header=\"540\" w:footer=\"540\" w:gutter=\"0\"/>\
         </w:sectPr>",
        twips(pw),
        twips(ph),
        m = MARGIN_TWIPS
    );
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {NAMESPACES}><w:body>{body}</w:body></w:document>\n"
    );
    let field = |instr: &str| {
        format!(
            "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> {instr} </w:instrText></w:r>\
             <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>"
        )
    };
    let footer = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:ftr {NAMESPACES}><w:p><w:pPr><w:pStyle w:val=\"Footer\"/></w:pPr>{}<w:r><w:tab/></w:r>{}{}{}{}</w:p></w:ftr>\n",
        run(&doc.title, ""),
        run("Page ", ""),
        field("PAGE"),
        run(" of ", ""),
        field("NUMPAGES")
    );
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
         <Relationship Id=\"rIdFooter\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>",
    );
    let mut media = Vec::new();
    for (i, p) in pictures.into_iter().enumerate() {
        let _ = write!(
            rels,
            "<Relationship Id=\"rIdPng{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/plot{i}.png\"/>\
             <Relationship Id=\"rIdSvg{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/plot{i}.svg\"/>"
        );
        media.push((format!("word/media/plot{i}.png"), p.png));
        media.push((format!("word/media/plot{i}.svg"), p.svg));
    }
    rels.push_str("</Relationships>\n");
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Default Extension=\"png\" ContentType=\"image/png\"/>\
         <Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
         <Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
         <Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>\
         <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
         <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
         </Types>\n";
    let package_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
         <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
         <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>\
         </Relationships>\n";
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>AnovaCraft</dc:creator>\
         </cp:coreProperties>\n",
        xml(&doc.title)
    );
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>AnovaCraft</Application></Properties>\n";
    let styles = STYLES.replace("TEXT_WIDTH", &twips(text_width_pt).to_string());
    let mut out = vec![
        ("[Content_Types].xml".to_owned(), content_types.as_bytes().to_vec()),
        ("_rels/.rels".to_owned(), package_rels.as_bytes().to_vec()),
        ("docProps/core.xml".to_owned(), core.into_bytes()),
        ("docProps/app.xml".to_owned(), app.as_bytes().to_vec()),
        ("word/document.xml".to_owned(), document.into_bytes()),
        ("word/styles.xml".to_owned(), styles.into_bytes()),
        ("word/footer1.xml".to_owned(), footer.into_bytes()),
        ("word/_rels/document.xml.rels".to_owned(), rels.into_bytes()),
    ];
    out.extend(media);
    out
}

/// `doc` as a `.docx` file.
///
/// # Errors
/// When the zip container cannot be written.
pub fn write(doc: &ReportDocument<'_>, options: ExportOptions) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let fail = |e: &dyn std::fmt::Display| format!("could not write the Word document: {e}");
    for (name, bytes) in parts(doc, options) {
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
    use crate::report_export::tests::{plant_growth, source};
    use crate::report_export::{ExportScope, PaperSize};

    #[test]
    fn escapes_xml() {
        assert_eq!(xml("a<b & \"c\"\u{1}"), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(
            runs("one\ntwo", ""),
            "<w:r><w:t xml:space=\"preserve\">one</w:t></w:r><w:r><w:br/><w:t xml:space=\"preserve\">two</w:t></w:r>"
        );
    }

    /// Every part is well-formed XML, the relationships point at parts that
    /// exist, and the document holds every heading and table cell.
    #[test]
    fn parts_are_well_formed_and_complete() {
        let (table, rows, computed) = plant_growth();
        let all: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
        let doc = ReportDocument::new(ExportScope::All(&all), source(&table, &rows));
        let parts = parts(&doc, ExportOptions { paper: PaperSize::A4 });
        let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
        for (name, bytes) in &parts {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let text = std::str::from_utf8(bytes).unwrap();
                roxmltree::Document::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
        let rels =
            std::str::from_utf8(&parts.iter().find(|(n, _)| n == "word/_rels/document.xml.rels").unwrap().1).unwrap();
        for target in rels.split("Target=\"").skip(1).map(|s| &s[..s.find('"').unwrap()]) {
            assert!(names.contains(&format!("word/{target}").as_str()), "{target}");
        }
        let document = std::str::from_utf8(&parts.iter().find(|(n, _)| n == "word/document.xml").unwrap().1).unwrap();
        assert!(document.contains("w:w=\"11906\" w:h=\"16838\""), "A4");
        for r in &computed {
            assert!(document.contains(&xml(&r.title)));
            for s in &r.sections {
                assert!(document.contains(&xml(&s.title)), "{}", s.title);
                if let crate::report::SectionContent::Table(t) = &s.content {
                    for cell in t.rows.iter().flatten().filter(|c| !c.is_empty()) {
                        assert!(document.contains(&format!(">{}<", xml(cell))), "{cell}");
                    }
                }
            }
        }
        assert!(names.iter().any(|n| n.ends_with(".png")) && names.iter().any(|n| n.ends_with(".svg")));
        let bytes = write(&doc, ExportOptions::default()).unwrap();
        assert!(bytes.starts_with(b"PK"));
    }

    /// When LibreOffice is installed, it opens the file and converts it to
    /// PDF (a check that a word processor reads it).
    #[test]
    #[ignore = "needs LibreOffice (soffice); run with --ignored"]
    fn libreoffice_opens_the_document() {
        let (table, rows, computed) = plant_growth();
        let doc = ReportDocument::new(ExportScope::One(HtmlReport::of(&computed[0])), source(&table, &rows));
        let dir = std::env::temp_dir().join(format!("anovacraft-docx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r.docx");
        std::fs::write(&path, write(&doc, ExportOptions::default()).unwrap()).unwrap();
        let status = std::process::Command::new("soffice")
            .args(["--headless", "--convert-to", "pdf", "--outdir"])
            .arg(&dir)
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(dir.join("r.pdf").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}

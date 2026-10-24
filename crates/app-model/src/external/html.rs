//! Tables on web pages: the `<table>` elements of an HTML document read as
//! rows of text, with `colspan` and `rowspan` cells repeated into every
//! position they cover (as the HTML table model lays them out).

/// One `<table>` of a page as a grid of cell text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTable {
    /// The table's `id`, if it has one.
    pub id: Option<String>,
    /// Column names from its heading rows (empty when it has none).
    pub header: Vec<String>,
    /// Body rows, all as wide as the widest row.
    pub rows: Vec<Vec<String>>,
}

/// Which table of a page to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableChoice<'a> {
    /// The table with this `id`, or the first table after the element with
    /// this `id` or `name` (a page section such as `#prices`); else the
    /// largest table.
    Anchor(&'a str),
    /// The n-th table, from 1.
    Number(usize),
    /// The largest table (most cells).
    Largest,
}

/// Longest `colspan` or `rowspan` honoured; larger values are clamped.
#[cfg_attr(not(feature = "web"), allow(dead_code))]
const MAX_SPAN: usize = 1000;

/// Whether a response looks like an HTML page rather than data.
#[must_use]
pub fn looks_like_html(text: &str, content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("html") {
        return true;
    }
    let start: String = text.trim_start().chars().take(512).collect::<String>().to_ascii_lowercase();
    start.starts_with("<!doctype html")
        || start.starts_with("<html")
        || (start.starts_with('<') && start.contains("<table"))
}

/// Every top-level-or-nested `<table>` of `html`, in document order, and the
/// index of the one `choice` picks (`None` when the page has no table, or a
/// numbered or named table is not there).
#[cfg(feature = "web")]
#[must_use]
pub fn page_tables(html: &str, choice: &TableChoice<'_>) -> (Vec<PageTable>, Option<usize>) {
    use scraper::{ElementRef, Html, Node};

    let doc = Html::parse_document(html);
    let mut tables = Vec::new();
    let mut table_nodes = Vec::new();
    let mut anchor_at: Option<usize> = None;
    let mut anchor_table: Option<usize> = None;
    let anchor = match choice {
        TableChoice::Anchor(a) => Some(*a),
        _ => None,
    };
    for node in doc.root_element().descendants() {
        let Node::Element(el) = node.value() else { continue };
        if el.name() == "table" {
            if let Some(e) = ElementRef::wrap(node) {
                table_nodes.push(node.id());
                tables.push(read_table(e));
            }
        }
        if let Some(a) = anchor {
            if anchor_at.is_none() && (el.id() == Some(a) || el.attr("name") == Some(a)) {
                anchor_at = Some(tables.len());
                // An anchor inside a table names that table.
                anchor_table = node
                    .ancestors()
                    .find(|n| matches!(n.value(), Node::Element(e) if e.name() == "table"))
                    .and_then(|t| table_nodes.iter().position(|&id| id == t.id()));
            }
        }
    }
    let largest = || {
        (0..tables.len())
            .filter(|&i| !tables[i].rows.is_empty())
            .max_by_key(|&i| (tables[i].rows.len() * tables[i].rows.first().map_or(0, Vec::len), usize::MAX - i))
    };
    let picked = match choice {
        TableChoice::Number(n) => n.checked_sub(1).filter(|&i| i < tables.len()),
        TableChoice::Largest => largest(),
        TableChoice::Anchor(a) => tables
            .iter()
            .position(|t| t.id.as_deref() == Some(*a))
            .or(anchor_table)
            .or_else(|| anchor_at.and_then(|at| (at..tables.len()).find(|&i| !tables[i].rows.is_empty())))
            .or_else(largest),
    };
    (tables, picked)
}

/// Rows of one table: `tr` elements whose nearest table is this one, laid
/// out on a grid that spanned cells fill.
#[cfg(feature = "web")]
fn read_table(table: scraper::ElementRef<'_>) -> PageTable {
    use scraper::{ElementRef, Node};

    let mut grid: Vec<Vec<Option<String>>> = Vec::new();
    // Whether each grid row is a heading row (in <thead>, or all <th>).
    let mut heading: Vec<bool> = Vec::new();
    let mut r = 0;
    for node in table.descendants() {
        let Some(tr) = ElementRef::wrap(node) else { continue };
        if tr.value().name() != "tr" {
            continue;
        }
        let owner = node.ancestors().find(|n| matches!(n.value(), Node::Element(e) if e.name() == "table"));
        if owner.map(|o| o.id()) != Some(table.id()) {
            continue;
        }
        let in_head = node.ancestors().any(|n| matches!(n.value(), Node::Element(e) if e.name() == "thead"));
        if grid.len() <= r {
            grid.resize_with(r + 1, Vec::new);
        }
        let mut all_th = true;
        let mut any_cell = false;
        let mut col = 0;
        for cell in tr.children().filter_map(ElementRef::wrap) {
            let name = cell.value().name();
            if name != "td" && name != "th" {
                continue;
            }
            any_cell = true;
            all_th &= name == "th";
            let span = |attr: &str| {
                cell.value().attr(attr).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, MAX_SPAN)
            };
            let (cols, rows) = (span("colspan"), span("rowspan"));
            let text = cell_text(cell);
            while grid[r].get(col).is_some_and(Option::is_some) {
                col += 1;
            }
            for dr in 0..rows {
                if r + dr >= grid.len() {
                    grid.resize_with(r + dr + 1, Vec::new);
                }
                let row = &mut grid[r + dr];
                if row.len() < col + cols {
                    row.resize(col + cols, None);
                }
                for c in col..col + cols {
                    row[c] = Some(text.clone());
                }
            }
            col += cols;
        }
        heading.resize(grid.len(), false);
        heading[r] = any_cell && (in_head || all_th);
        r += 1;
    }
    heading.resize(grid.len(), false);
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    let full = |row: Vec<Option<String>>| -> Vec<String> {
        let mut out: Vec<String> = row.into_iter().map(Option::unwrap_or_default).collect();
        out.resize(width, String::new());
        out
    };
    let mut header_rows = Vec::new();
    let mut rows = Vec::new();
    for (row, is_heading) in grid.into_iter().zip(heading) {
        let row = full(row);
        if row.iter().all(|c| c.is_empty()) {
            continue;
        }
        if is_heading && rows.is_empty() {
            header_rows.push(row);
        } else {
            rows.push(row);
        }
    }
    let header = (0..width)
        .map(|c| {
            let mut parts: Vec<&str> = Vec::new();
            for h in &header_rows {
                let p = h[c].as_str();
                if !p.is_empty() && parts.last() != Some(&p) {
                    parts.push(p);
                }
            }
            parts.join(" ")
        })
        .collect::<Vec<_>>();
    let header = if header.iter().all(String::is_empty) { Vec::new() } else { header };
    PageTable { id: table.value().id().map(str::to_owned), header, rows }
}

/// A cell's text with runs of white space (line breaks included) made one
/// space.
#[cfg(feature = "web")]
fn cell_text(cell: scraper::ElementRef<'_>) -> String {
    cell.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Unique, non-empty column names (`Column 3` for a blank one, `Price 2`
/// for a second `Price`).
#[must_use]
pub fn column_names(header: &[String], width: usize) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(width);
    for c in 0..width {
        let base = header
            .get(c)
            .map(|h| h.trim())
            .filter(|h| !h.is_empty())
            .map_or_else(|| format!("Column {}", c + 1), str::to_owned);
        let mut name = base.clone();
        let mut k = 2;
        while names.contains(&name) {
            name = format!("{base} {k}");
            k += 1;
        }
        names.push(name);
    }
    names
}

#[cfg(all(test, feature = "web"))]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!DOCTYPE html><html><body>
        <table id="nav"><tr><td>Home</td><td>About</td></tr></table>
        <h2 id="1D">Daily lenses</h2>
        <table>
          <thead><tr><th rowspan="2">Lens</th><th colspan="2">Power</th></tr>
                 <tr><th>Min</th><th>Max</th></tr></thead>
          <tbody>
            <tr><td>Alpha &amp; Co</td><td>-12.00</td><td>+8.00</td></tr>
            <tr><td rowspan="2">Beta</td><td>-10</td><td>
                 6</td></tr>
            <tr><td>-9</td><td>5</td></tr>
          </tbody>
        </table>
        <table id="big"><tr><th>A</th><th>A</th><th></th></tr>
          <tr><td>1</td><td>2</td><td>3</td></tr><tr><td>4</td><td>5</td><td>6</td></tr><tr><td>0</td><td>0</td><td>0</td></tr>
          <tr><td>7</td><td>8<table><tr><td>nested</td></tr></table></td><td>9</td></tr></table>
        </body></html>"#;

    #[test]
    fn spans_fill_the_grid_and_heading_rows_combine() {
        let (tables, picked) = page_tables(PAGE, &TableChoice::Anchor("1D"));
        assert_eq!(picked, Some(1), "the first table after the #1D heading");
        let t = &tables[1];
        assert_eq!(t.header, ["Lens", "Power Min", "Power Max"]);
        assert_eq!(
            t.rows,
            [["Alpha & Co", "-12.00", "+8.00"], ["Beta", "-10", "6"], ["Beta", "-9", "5"]].map(|r| r.map(String::from))
        );
    }

    #[test]
    fn tables_are_chosen_by_id_number_or_size() {
        assert_eq!(page_tables(PAGE, &TableChoice::Anchor("big")).1, Some(2));
        assert_eq!(page_tables(PAGE, &TableChoice::Number(1)).1, Some(0));
        assert_eq!(page_tables(PAGE, &TableChoice::Number(9)).1, None);
        assert_eq!(page_tables(PAGE, &TableChoice::Largest).1, Some(2));
        // An unknown anchor falls back to the largest table.
        assert_eq!(page_tables(PAGE, &TableChoice::Anchor("nowhere")).1, Some(2));
        // The nested table is listed but does not add rows to its parent.
        let (tables, _) = page_tables(PAGE, &TableChoice::Largest);
        assert_eq!(tables.len(), 4);
        assert_eq!(tables[2].rows.len(), 4);
        assert_eq!(tables[2].rows[3], ["7", "8nested", "9"]);
        assert!(page_tables("<p>no tables</p>", &TableChoice::Largest).1.is_none());
    }

    #[test]
    fn names_are_unique_and_never_blank() {
        assert_eq!(column_names(&["A".into(), "A".into(), String::new()], 4), ["A", "A 2", "Column 3", "Column 4"]);
    }

    #[test]
    fn html_is_recognised() {
        assert!(looks_like_html("x", "text/html; charset=utf-8"));
        assert!(looks_like_html("  <!DOCTYPE html><p>", ""));
        assert!(!looks_like_html("a,b\n1,2", "text/csv"));
        assert!(!looks_like_html("[{\"a\": \"<table>\"}]", "application/json"));
    }
}

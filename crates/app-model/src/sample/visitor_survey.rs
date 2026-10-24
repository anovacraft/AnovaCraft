//! Sample for multiple-response (check-all-that-apply) answers.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{ItemOrder, MultipleResponseRequest, ResponseSource};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};

/// What visitors could tick, with the percent chance of a tick in each age
/// group (under 35, 35–54, 55 and over).
const SOURCES: [(&str, [usize; 3]); 6] = [
    ("Friends", [55, 45, 40]),
    ("Social media", [70, 40, 12]),
    ("Website", [50, 55, 35]),
    ("Newspaper", [5, 15, 45]),
    ("Radio", [10, 25, 30]),
    ("Poster", [20, 20, 20]),
];

/// Age groups and how many visitors are in each.
const AGES: [(&str, usize); 3] = [("Under 35", 60), ("35–54", 55), ("55 and over", 45)];

/// Things a visitor could write in the comment box.
const COMMENTS: [&str; 6] = [
    "",
    "Loved the new gallery.",
    "The café was busy at lunchtime, and we waited a long time for a table. The exhibits themselves were \
     excellent, especially the hands-on room, which kept the children busy for most of the afternoon.",
    "Parking was hard to find.",
    "",
    "Please open earlier on weekends. We came on a Sunday morning and queued outside for twenty minutes; \
     a members' entrance would help regular visitors.",
];

/// A **simulated** visitor survey: 160 visitors in three age groups ticked
/// every way they had heard of a museum's exhibition (a check-all-that-apply
/// question), stored as one text column of items separated by `"; "`. Each
/// item is ticked independently with a chance that depends on the age group
/// (social media far more often by the young, newspapers by the old;
/// posters equally); a visitor who ticked nothing is recorded with an empty
/// cell. Also a 1–5 rating and a free-text comment, some of it long. Drawn
/// with the workspace's integer-based generator (seed `0x5EED`), so the
/// table is the same on every platform.
#[must_use]
pub fn visitor_survey() -> DataTable {
    let mut rng = Rng::new(0x5EED);
    let (mut id, mut age, mut heard, mut rating, mut comment) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (g, (label, n)) in AGES.iter().enumerate() {
        for _ in 0..*n {
            id.push(format!("V{:03}", id.len() + 1));
            age.push((*label).to_owned());
            let ticked: Vec<&str> =
                SOURCES.iter().filter(|(_, p)| rng.below(100) < p[g]).map(|(item, _)| *item).collect();
            heard.push(ticked.join("; "));
            rating.push((1 + rng.below(5)) as f64);
            comment.push(COMMENTS[rng.below(COMMENTS.len())].to_owned());
        }
    }
    table(
        "Visitor survey (simulated)",
        vec![
            Column::text("Visitor", id),
            Column::text("Age group", age),
            Column::text("Heard about it from", heard),
            Column::numeric("Rating", rating),
            Column::text("Comment", comment),
        ],
    )
}

pub(super) fn visitor_survey_sample() -> Sample {
    let t = visitor_survey();
    let reports = vec![SavedReport::MultipleResponseItems {
        request: MultipleResponseRequest {
            source: ResponseSource::Lists { column: col(&t, "Heard about it from"), delimiter: ";".to_owned() },
            group: Some(col(&t, "Age group")),
            order: ItemOrder::Frequency,
        },
    }];
    Sample { table: t, reports }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MultipleResponseAnalysis;
    use stat_data::RowStateManager;

    /// Counts by hand: every item's tick count equals the number of cells
    /// whose split list holds it, and the shares follow from them.
    #[test]
    fn the_survey_counts_match_a_hand_count() {
        let t = visitor_survey();
        assert_eq!(t.n_rows(), 160);
        let lists = t.column(col(&t, "Heard about it from")).unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let sample = visitor_survey_sample();
        let SavedReport::MultipleResponseItems { request } = &sample.reports[0] else { panic!() };
        let a = MultipleResponseAnalysis::run(&t, &rows, request).unwrap();
        let sections = a.sections();
        let freq = sections.iter().find(|s| s.title.starts_with("Response Frequencies")).expect("frequency table");
        let crate::report::SectionContent::Table(table) = &freq.content else { panic!() };
        for (item, _) in SOURCES {
            let hand = (0..t.n_rows()).filter(|&r| lists.cell_text(r).split("; ").any(|i| i == item)).count();
            let row = table.rows.iter().find(|r| r[0] == item).expect("item row");
            assert_eq!(row[1], hand.to_string(), "{item}");
        }
    }
}

//! Checks the Porter stemmer against the output of Martin Porter's reference
//! implementation for its sample vocabulary (23,531 words, line-aligned).

use stat_text::porter::porter_stem;

#[test]
fn matches_reference_implementation_on_sample_vocabulary() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");
    let vocabulary =
        std::fs::read_to_string(format!("{dir}/porter_vocabulary.txt")).expect("read porter_vocabulary.txt");
    let expected =
        std::fs::read_to_string(format!("{dir}/porter_martin_output.txt")).expect("read porter_martin_output.txt");
    let words: Vec<&str> = vocabulary.lines().collect();
    let stems: Vec<&str> = expected.lines().collect();
    assert_eq!(words.len(), 23_531, "vocabulary size");
    assert_eq!(words.len(), stems.len(), "files are line-aligned");

    let mismatches: Vec<String> = words
        .iter()
        .zip(&stems)
        .enumerate()
        .filter_map(|(line, (word, stem))| {
            let got = porter_stem(word);
            (got != *stem).then(|| format!("line {}: {word} -> {got}, expected {stem}", line + 1))
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} words differ; first mismatches:\n{}",
        mismatches.len(),
        words.len(),
        mismatches.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

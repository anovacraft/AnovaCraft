//! Help ▸ About ▸ Open-source licences: the copyright notices and licence
//! texts of the fonts, icons, data and Rust crates built into the app.
//!
//! The text is `THIRD-PARTY-NOTICES.txt` at the repository root, written by
//! `scripts/build-notices.py` from Cargo.lock and the crates' own licence
//! files, and embedded so it travels with the executable.

use eframe::egui;

/// The notices file, embedded at build time.
pub(crate) const THIRD_PARTY_NOTICES: &str = include_str!("../../../../THIRD-PARTY-NOTICES.txt");

/// The notices as a scrolling list of monospaced lines (only the visible
/// lines are laid out, so the long file costs nothing to show).
pub(super) fn show(ui: &mut egui::Ui) {
    ui.label("AnovaCraft is built with these open-source fonts, icons, data and libraries, under the licences below.");
    ui.add_space(4.0);
    let lines: Vec<&str> = THIRD_PARTY_NOTICES.lines().collect();
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both().auto_shrink([false, false]).show_rows(ui, row_height, lines.len(), |ui, rows| {
        for line in &lines[rows] {
            ui.add(egui::Label::new(egui::RichText::new(*line).monospace()).extend());
        }
    });
}

/// FNV-1a 64-bit hash of Cargo.lock without carriage returns, as
/// `scripts/build-notices.py` records it in the notices file.
#[cfg(test)]
fn lock_fingerprint(lock: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in lock.iter().filter(|&&b| b != b'\r') {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_party_notices_match_cargo_lock() {
        let lock = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")).unwrap();
        let expected = format!("(fingerprint {})", lock_fingerprint(&lock));
        assert!(
            THIRD_PARTY_NOTICES.contains(&expected),
            "Cargo.lock changed since THIRD-PARTY-NOTICES.txt was written: run python3 scripts/build-notices.py \
             and commit the file"
        );
    }

    #[test]
    fn notices_cover_the_bundled_fonts_icons_and_data() {
        for name in
            ["Inter", "JetBrains Mono", "Liberation", "DejaVu", "Hack", "Ubuntu", "Noto Emoji", "Lucide", "VADER"]
        {
            assert!(THIRD_PARTY_NOTICES.contains(name), "{name}");
        }
        for name in ["egui 0.31.1", "rhai ", "rayon ", "SIL OPEN FONT LICENSE"] {
            assert!(THIRD_PARTY_NOTICES.contains(name), "{name}");
        }
    }
}

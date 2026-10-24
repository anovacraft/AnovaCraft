//! Random seeds as saved in project and pipeline files.
//!
//! Reports save their seeds as JSON numbers, which hold whole numbers
//! exactly only up to 2⁵³. Seeds are therefore kept at or below
//! [`MAX_SEED`]: dialogs cap entry there, and a seed read from a file or a
//! script is clamped to it, so every seed in use saves and reopens exactly.

use stat_data::JsonValue;

/// The largest seed: 2⁵³ − 1, the largest whole number every JSON reader
/// holds exactly.
pub const MAX_SEED: u64 = (1 << 53) - 1;

/// `seed`, capped at [`MAX_SEED`].
#[must_use]
pub fn clamp_seed(seed: u64) -> u64 {
    seed.min(MAX_SEED)
}

/// The seed held by `value`: a non-negative number, its fraction dropped and
/// capped at [`MAX_SEED`]. `None` for anything else (a negative number,
/// text, a missing value).
#[must_use]
pub fn seed_from_json(value: &JsonValue) -> Option<u64> {
    let x = value.as_f64()?;
    // Saturating float-to-integer conversion: anything above 2⁶⁴ becomes
    // u64::MAX before the cap.
    (x.is_finite() && x >= 0.0).then(|| clamp_seed(x.trunc() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every seed a dialog accepts survives being written and read back.
    #[test]
    fn seeds_round_trip_through_json_text() {
        for seed in [0, 1, 42, 1 << 32, MAX_SEED - 1, MAX_SEED] {
            let text = JsonValue::Number(seed as f64).to_compact();
            let back = stat_data::parse_json(&text).expect("valid JSON");
            assert_eq!(seed_from_json(&back), Some(seed), "{seed} through {text}");
        }
        // Above the cap, f64 would round: 2⁵³ + 1 is stored as 2⁵³.
        assert_ne!(((MAX_SEED + 2) as f64) as u64, MAX_SEED + 2);
    }

    #[test]
    fn large_and_invalid_seeds_are_clamped_or_refused() {
        assert_eq!(clamp_seed(u64::MAX), MAX_SEED);
        assert_eq!(clamp_seed(7), 7);
        assert_eq!(seed_from_json(&JsonValue::Number(1e30)), Some(MAX_SEED));
        assert_eq!(seed_from_json(&JsonValue::Number(2f64.powi(53))), Some(MAX_SEED));
        assert_eq!(seed_from_json(&JsonValue::Number(12.7)), Some(12));
        assert_eq!(seed_from_json(&JsonValue::Number(-1.0)), None);
        assert_eq!(seed_from_json(&JsonValue::Number(f64::NAN)), None);
        assert_eq!(seed_from_json(&JsonValue::String("5".into())), None);
    }
}

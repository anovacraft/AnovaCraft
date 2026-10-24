//! Small helpers for writing mathematical notation in report text.

/// `text` in superscript characters (digits, signs, parentheses and `n`),
/// for exponents such as `2⁽⁵⁻¹⁾` or `x²`. Characters without a superscript
/// form are kept as they are.
#[must_use]
pub fn superscript(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '-' | '−' => '⁻',
            '+' => '⁺',
            '(' => '⁽',
            ')' => '⁾',
            'n' => 'ⁿ',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponents() {
        assert_eq!(superscript("10"), "¹⁰");
        assert_eq!(superscript("(5−1)"), "⁽⁵⁻¹⁾");
        assert_eq!(superscript("k"), "k");
    }
}

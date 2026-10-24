//! The Porter (1980) suffix-stripping stemmer.
//!
//! Porter, M.F. (1980), "An algorithm for suffix stripping", *Program*
//! 14(3), 130–137, reduces an English word to a stem by removing suffixes in
//! five ordered steps. This module follows the rules in the form of Martin
//! Porter's own reference implementation, which departs from the published
//! paper in two documented places, both in step 2:
//!
//! * `bli` → `ble` replaces the paper's `abli` → `able`;
//! * the rule `logi` → `log` is added.
//!
//! ## Definitions
//!
//! A *consonant* is a letter other than `a e i o u`, and other than `y`
//! when `y` follows a consonant. Writing `C` for a run of consonants and `V`
//! for a run of vowels, every word has the form `[C](VC)^m[V]`; `m` is the
//! *measure* of the stem. The rule conditions are:
//!
//! * `m > n` — the stem left after removing the suffix has measure above `n`;
//! * `*v*` — the stem contains a vowel;
//! * `*d` — the stem ends in a double consonant;
//! * `*o` — the stem ends consonant–vowel–consonant, the last consonant not
//!   `w`, `x` or `y`.
//!
//! The stemmer is verified against the reference implementation's output
//! for its 23,531-word sample vocabulary (`tests/porter_reference.rs`).

/// Stem one lower-case word with Porter's algorithm.
///
/// Applies steps 1a (plurals), 1b (`-ed`, `-ing`), 1c (`y` → `i`), 2 and 3
/// (double and single derivational suffixes mapped to shorter ones), 4
/// (suffixes removed when the stem measure exceeds 1) and 5 (a final `e` and
/// a double `l`), as described in the [module documentation](self).
///
/// # Inputs
///
/// `word` is expected to be a single lower-case word. Words of two letters or
/// fewer are returned unchanged, as are words containing any character
/// outside the ASCII letters `a`–`z` (upper-case letters, digits,
/// apostrophes, hyphens or non-ASCII letters), since the rules are defined
/// only for lower-case English letters.
///
/// # Failure modes
///
/// None: the function is total and never panics.
pub fn porter_stem(word: &str) -> String {
    if word.len() <= 2 || !word.bytes().all(|c| c.is_ascii_lowercase()) {
        return word.to_owned();
    }
    let mut stemmer = Stemmer::new(word.as_bytes());
    stemmer.run();
    stemmer.into_string()
}

/// Working state: the letters, the current end `k` (inclusive) and the
/// stem end `j` (inclusive) set by the last successful [`Stemmer::ends`].
/// Both indices are signed because a suffix may cover the whole word, which
/// leaves `j = -1` (an empty stem).
struct Stemmer {
    b: Vec<u8>,
    k: isize,
    j: isize,
}

impl Stemmer {
    fn new(word: &[u8]) -> Self {
        let k = word.len() as isize - 1;
        Self { b: word.to_vec(), k, j: k }
    }

    fn into_string(mut self) -> String {
        self.b.truncate((self.k + 1) as usize);
        // Only ASCII letters are ever written, so the buffer is valid UTF-8.
        String::from_utf8(self.b).unwrap_or_default()
    }

    fn at(&self, i: isize) -> u8 {
        self.b[i as usize]
    }

    /// Whether the letter at `i` is a consonant (`y` is a consonant at the
    /// start of the word or after a vowel).
    fn cons(&self, i: isize) -> bool {
        match self.at(i) {
            b'a' | b'e' | b'i' | b'o' | b'u' => false,
            b'y' => i == 0 || !self.cons(i - 1),
            _ => true,
        }
    }

    /// The measure `m` of the stem `b[0..=j]`: the number of VC sequences.
    fn m(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        let j = self.j;
        // Skip the leading consonants.
        loop {
            if i > j {
                return n;
            }
            if !self.cons(i) {
                break;
            }
            i += 1;
        }
        i += 1;
        loop {
            // Skip vowels.
            loop {
                if i > j {
                    return n;
                }
                if self.cons(i) {
                    break;
                }
                i += 1;
            }
            i += 1;
            n += 1;
            // Skip consonants.
            loop {
                if i > j {
                    return n;
                }
                if !self.cons(i) {
                    break;
                }
                i += 1;
            }
            i += 1;
        }
    }

    /// Condition `*v*`: the stem `b[0..=j]` contains a vowel.
    fn vowel_in_stem(&self) -> bool {
        (0..=self.j).any(|i| !self.cons(i))
    }

    /// Condition `*d`: `b[i-1..=i]` is a double consonant.
    fn double_c(&self, i: isize) -> bool {
        i >= 1 && self.at(i) == self.at(i - 1) && self.cons(i)
    }

    /// Condition `*o`: `b[i-2..=i]` is consonant–vowel–consonant and the
    /// last letter is not `w`, `x` or `y`.
    fn cvc(&self, i: isize) -> bool {
        if i < 2 || !self.cons(i) || self.cons(i - 1) || !self.cons(i - 2) {
            return false;
        }
        !matches!(self.at(i), b'w' | b'x' | b'y')
    }

    /// Whether `b[0..=k]` ends with `s`; on success sets `j` to the end of
    /// the stem before the suffix.
    fn ends(&mut self, s: &[u8]) -> bool {
        let len = s.len() as isize;
        if len > self.k + 1 {
            return false;
        }
        let start = (self.k + 1 - len) as usize;
        if &self.b[start..=self.k as usize] != s {
            return false;
        }
        self.j = self.k - len;
        true
    }

    /// Replaces `b[j+1..=k]` with `s` and moves `k` to the new end.
    fn set_to(&mut self, s: &[u8]) {
        let start = (self.j + 1) as usize;
        self.b.truncate(start);
        self.b.extend_from_slice(s);
        self.k = self.j + s.len() as isize;
    }

    /// Replaces the suffix with `s` when the stem's measure is positive.
    fn replace_if_m_positive(&mut self, s: &[u8]) {
        if self.m() > 0 {
            self.set_to(s);
        }
    }

    /// Tries `(suffix, replacement)` rules in order; the first suffix that
    /// matches is replaced when `m > 0`, and no later rule is tried.
    fn first_rule(&mut self, rules: &[(&[u8], &[u8])]) {
        for (suffix, replacement) in rules {
            if self.ends(suffix) {
                self.replace_if_m_positive(replacement);
                return;
            }
        }
    }

    fn run(&mut self) {
        self.step1ab();
        if self.k > 0 {
            self.step1c();
            self.step2();
            self.step3();
            self.step4();
            self.step5();
        }
    }

    /// Step 1a removes plurals; step 1b removes `-eed`, `-ed` and `-ing`
    /// and tidies the stem that is left.
    fn step1ab(&mut self) {
        if self.at(self.k) == b's' {
            if self.ends(b"sses") {
                self.k -= 2;
            } else if self.ends(b"ies") {
                self.set_to(b"i");
            } else if self.at(self.k - 1) != b's' {
                self.k -= 1;
            }
        }
        if self.ends(b"eed") {
            if self.m() > 0 {
                self.k -= 1;
            }
        } else if (self.ends(b"ed") || self.ends(b"ing")) && self.vowel_in_stem() {
            self.k = self.j;
            if self.ends(b"at") {
                self.set_to(b"ate");
            } else if self.ends(b"bl") {
                self.set_to(b"ble");
            } else if self.ends(b"iz") {
                self.set_to(b"ize");
            } else if self.double_c(self.k) {
                self.k -= 1;
                if matches!(self.at(self.k), b'l' | b's' | b'z') {
                    self.k += 1;
                }
            } else if self.m() == 1 && self.cvc(self.k) {
                self.set_to(b"e");
            }
        }
    }

    /// Step 1c turns a final `y` into `i` when the stem has a vowel.
    fn step1c(&mut self) {
        if self.ends(b"y") && self.vowel_in_stem() {
            let k = self.k as usize;
            self.b[k] = b'i';
        }
    }

    /// Step 2 maps double suffixes to single ones when `m > 0`.
    fn step2(&mut self) {
        let rules: &[(&[u8], &[u8])] = match self.at(self.k - 1) {
            b'a' => &[(b"ational", b"ate"), (b"tional", b"tion")],
            b'c' => &[(b"enci", b"ence"), (b"anci", b"ance")],
            b'e' => &[(b"izer", b"ize")],
            b'l' => &[(b"bli", b"ble"), (b"alli", b"al"), (b"entli", b"ent"), (b"eli", b"e"), (b"ousli", b"ous")],
            b'o' => &[(b"ization", b"ize"), (b"ation", b"ate"), (b"ator", b"ate")],
            b's' => &[(b"alism", b"al"), (b"iveness", b"ive"), (b"fulness", b"ful"), (b"ousness", b"ous")],
            b't' => &[(b"aliti", b"al"), (b"iviti", b"ive"), (b"biliti", b"ble")],
            b'g' => &[(b"logi", b"log")],
            _ => return,
        };
        self.first_rule(rules);
    }

    /// Step 3 handles `-ic-`, `-full`, `-ness` and similar when `m > 0`.
    fn step3(&mut self) {
        let rules: &[(&[u8], &[u8])] = match self.at(self.k) {
            b'e' => &[(b"icate", b"ic"), (b"ative", b""), (b"alize", b"al")],
            b'i' => &[(b"iciti", b"ic")],
            b'l' => &[(b"ical", b"ic"), (b"ful", b"")],
            b's' => &[(b"ness", b"")],
            _ => return,
        };
        self.first_rule(rules);
    }

    /// Step 4 removes a suffix when the remaining stem has `m > 1`.
    fn step4(&mut self) {
        let suffixes: &[&[u8]] = match self.at(self.k - 1) {
            b'a' => &[b"al"],
            b'c' => &[b"ance", b"ence"],
            b'e' => &[b"er"],
            b'i' => &[b"ic"],
            b'l' => &[b"able", b"ible"],
            b'n' => &[b"ant", b"ement", b"ment", b"ent"],
            b'o' => {
                // `-ion` only after `s` or `t`; otherwise try `-ou`.
                let ion = self.ends(b"ion") && self.j >= 0 && matches!(self.at(self.j), b's' | b't');
                if !ion && !self.ends(b"ou") {
                    return;
                }
                &[]
            }
            b's' => &[b"ism"],
            b't' => &[b"ate", b"iti"],
            b'u' => &[b"ous"],
            b'v' => &[b"ive"],
            b'z' => &[b"ize"],
            _ => return,
        };
        if !suffixes.is_empty() && !suffixes.iter().any(|s| self.ends(s)) {
            return;
        }
        if self.m() > 1 {
            self.k = self.j;
        }
    }

    /// Step 5 removes a final `e` (when `m > 1`, or `m = 1` and not `*o`)
    /// and reduces a final `ll` to `l` when `m > 1`.
    fn step5(&mut self) {
        self.j = self.k;
        if self.at(self.k) == b'e' {
            let a = self.m();
            if a > 1 || (a == 1 && !self.cvc(self.k - 1)) {
                self.k -= 1;
            }
        }
        if self.at(self.k) == b'l' && self.double_c(self.k) && self.m() > 1 {
            self.k -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::porter_stem;

    #[test]
    fn classic_examples_from_the_paper() {
        let cases = [
            ("caresses", "caress"),
            ("ponies", "poni"),
            ("ties", "ti"),
            ("caress", "caress"),
            ("cats", "cat"),
            ("feed", "feed"),
            ("agreed", "agre"),
            ("plastered", "plaster"),
            ("bled", "bled"),
            ("motoring", "motor"),
            ("sing", "sing"),
            ("conflated", "conflat"),
            ("troubled", "troubl"),
            ("sized", "size"),
            ("hopping", "hop"),
            ("tanned", "tan"),
            ("falling", "fall"),
            ("hissing", "hiss"),
            ("fizzed", "fizz"),
            ("failing", "fail"),
            ("filing", "file"),
            ("happy", "happi"),
            ("sky", "sky"),
            ("relational", "relat"),
            ("conditional", "condit"),
            ("rational", "ration"),
            ("digitizer", "digit"),
            ("generalization", "gener"),
            ("generalizations", "gener"),
            ("oscillators", "oscil"),
            ("hopeful", "hope"),
            ("goodness", "good"),
            ("adjustment", "adjust"),
            ("adoption", "adopt"),
            ("controll", "control"),
            ("roll", "roll"),
            ("probate", "probat"),
            ("rate", "rate"),
            ("cease", "ceas"),
        ];
        for (word, stem) in cases {
            assert_eq!(porter_stem(word), stem, "stem of {word}");
        }
    }

    #[test]
    fn reference_implementation_departures() {
        // "logi" -> "log" and "bli" -> "ble" in step 2.
        assert_eq!(porter_stem("archaeology"), "archaeolog");
        assert_eq!(porter_stem("possibly"), "possibl");
    }

    #[test]
    fn short_and_non_letter_words_are_unchanged() {
        assert_eq!(porter_stem(""), "");
        assert_eq!(porter_stem("is"), "is");
        assert_eq!(porter_stem("as"), "as");
        assert_eq!(porter_stem("Running"), "Running");
        assert_eq!(porter_stem("don't"), "don't");
        assert_eq!(porter_stem("covid19"), "covid19");
        assert_eq!(porter_stem("naïve"), "naïve");
    }

    #[test]
    fn suffix_covering_the_whole_word() {
        assert_eq!(porter_stem("ies"), "i");
        assert_eq!(porter_stem("ion"), "ion");
        assert_eq!(porter_stem("eed"), "eed");
    }
}

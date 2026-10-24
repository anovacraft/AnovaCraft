//! The typeset view of a formula: a column formula written in Rhai, drawn
//! as mathematics (fractions stacked over a rule, powers raised, square
//! roots under a radical sign, sums as Σ, a row's neighbours as subscripts).
//!
//! [`typeset`] reads one expression with the operator precedence of the
//! formula language (Rhai: `||` below `&&` below `==` `!=` below `<` `<=`
//! `>` `>=` below `+` `-` below `*` `/` `%` below `**`, which groups to the
//! right; prefix `-` and `!` bind tighter than every binary operator, so
//! `-x ** 2` is (−x)²) and writes it in the label markup of
//! [`stat_viz::math_text`]. Parentheses are dropped where the layout makes
//! them unnecessary (inside a fraction, an exponent, a root or a function's
//! arguments) and added where the precedence needs them.
//!
//! Only expressions are typeset: a formula with statements (`let`, `;`
//! between steps), conditions (`if`, `switch`), method calls or anything
//! else outside the arithmetic, comparison and function-call subset gives
//! `None`, and the editor shows no typeset view for it.

use crate::formula::COLUMN_FUNCTIONS;

/// Deepest nesting read before giving up (a guard on recursion).
const MAX_DEPTH: usize = 200;

/// `formula` as label markup for [`stat_viz::math_text::MathText::parse`],
/// or `None` when it is not a single expression of the typeset subset (see
/// the module documentation).
#[must_use]
pub fn typeset(formula: &str) -> Option<String> {
    let mut tokens = tokenize(formula)?;
    // One trailing semicolon ends the expression.
    if tokens.last() == Some(&Token::Semicolon) {
        tokens.pop();
    }
    if tokens.is_empty() {
        return None;
    }
    let mut p = Parser { tokens, pos: 0, depth: 0 };
    let e = p.expression(0)?;
    if p.pos != p.tokens.len() {
        return None;
    }
    Some(render(&e).text)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(String),
    Ident(String),
    Str(String),
    Op(&'static str),
    Open,
    Close,
    Comma,
    Semicolon,
}

/// Operators, longest first so `**` is read before `*`.
const OPERATORS: &[&str] = &["**", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "%", "<", ">", "!"];

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                // A sign directly after the exponent's `e`.
                if (chars[i] == 'e' || chars[i] == 'E') && matches!(chars.get(i + 1), Some('+' | '-')) {
                    i += 1;
                }
                i += 1;
            }
            let number: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            number.parse::<f64>().ok()?;
            out.push(Token::Number(number));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Ident(chars[start..i].iter().collect()));
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match chars.get(i)? {
                    '"' => break,
                    '\\' => {
                        i += 1;
                        s.push(match chars.get(i)? {
                            'n' => '\n',
                            't' => '\t',
                            other => *other,
                        });
                    }
                    other => s.push(*other),
                }
                i += 1;
            }
            i += 1;
            out.push(Token::Str(s));
        } else if c == '(' {
            out.push(Token::Open);
            i += 1;
        } else if c == ')' {
            out.push(Token::Close);
            i += 1;
        } else if c == ',' {
            out.push(Token::Comma);
            i += 1;
        } else if c == ';' {
            out.push(Token::Semicolon);
            i += 1;
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = OPERATORS.iter().find(|op| rest.starts_with(**op))?;
            // `//` starts a comment, `/*` a block comment: not typeset.
            if *op == "/" && matches!(chars.get(i + 1), Some('/' | '*')) {
                return None;
            }
            i += op.chars().count();
            out.push(Token::Op(op));
        }
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Number(String),
    Ident(String),
    Str(String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

/// Binding power of a binary operator, and whether it groups to the right.
fn binding(op: &str) -> Option<(u8, bool)> {
    Some(match op {
        "||" => (30, false),
        "&&" => (60, false),
        "==" | "!=" => (90, false),
        "<" | "<=" | ">" | ">=" => (110, false),
        "+" | "-" => (150, false),
        "*" | "/" | "%" => (180, false),
        "**" => (190, true),
        _ => return None,
    })
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    /// Precedence climbing: binary operators binding tighter than `min`.
    fn expression(&mut self, min: u8) -> Option<Expr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let mut left = self.unary()?;
        while let Some(Token::Op(op)) = self.peek() {
            let op = *op;
            let Some((power, right)) = binding(op) else { break };
            if power <= min {
                break;
            }
            self.pos += 1;
            let rhs = self.expression(if right { power - 1 } else { power })?;
            left = Expr::Binary(op, Box::new(left), Box::new(rhs));
        }
        self.depth -= 1;
        Some(left)
    }

    fn unary(&mut self) -> Option<Expr> {
        match self.peek()? {
            Token::Op(op @ ("-" | "+" | "!")) => {
                let op = *op;
                self.pos += 1;
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return None;
                }
                let operand = self.unary()?;
                self.depth -= 1;
                Some(if op == "+" { operand } else { Expr::Unary(op, Box::new(operand)) })
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Option<Expr> {
        match self.next()? {
            Token::Number(n) => Some(Expr::Number(n)),
            Token::Str(s) => Some(Expr::Str(s)),
            Token::Open => {
                let e = self.expression(0)?;
                (self.next()? == Token::Close).then_some(e)
            }
            Token::Ident(name) => {
                if matches!(
                    name.as_str(),
                    "let" | "const" | "if" | "else" | "switch" | "while" | "loop" | "for" | "in" | "fn" | "return"
                ) {
                    return None;
                }
                if self.peek() != Some(&Token::Open) {
                    return Some(Expr::Ident(name));
                }
                self.pos += 1;
                let mut args = Vec::new();
                if self.peek() == Some(&Token::Close) {
                    self.pos += 1;
                } else {
                    loop {
                        args.push(self.expression(0)?);
                        match self.next()? {
                            Token::Comma => {}
                            Token::Close => break,
                            _ => return None,
                        }
                    }
                }
                Some(Expr::Call(name, args))
            }
            _ => None,
        }
    }
}

/// How a rendered piece groups, for deciding where parentheses go (from
/// loosest to tightest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Or,
    And,
    Equality,
    Comparison,
    Sum,
    Product,
    Prefix,
    Power,
    /// A fraction: delimited by its rule, but not a power's base.
    Fraction,
    /// A name, number, call, root or absolute value.
    Atom,
}

struct Piece {
    text: String,
    kind: Kind,
}

fn piece(text: impl Into<String>, kind: Kind) -> Piece {
    Piece { text: text.into(), kind }
}

/// `p` in parentheses when `wrap`.
fn paren(p: Piece, wrap: bool) -> String {
    if wrap { format!("({})", p.text) } else { p.text }
}

/// Text with the markup's special characters made literal (for a column
/// name shown beside a typeset formula).
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '_' | '^' | '{' | '}' | '$' | '/') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A number as written, with an exponent shown as a power of ten.
fn number(n: &str) -> Piece {
    match n.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => {
            let (sign, digits) = match exponent.strip_prefix('-') {
                Some(d) => ("−", d),
                None => ("", exponent.trim_start_matches('+')),
            };
            let digits = digits.trim_start_matches('0');
            let exponent = if digits.is_empty() { "0".to_owned() } else { format!("{sign}{digits}") };
            if mantissa == "1" {
                piece(format!("10^{{{exponent}}}"), Kind::Power)
            } else {
                piece(format!("{mantissa} \\times 10^{{{exponent}}}"), Kind::Product)
            }
        }
        None => piece(n, Kind::Atom),
    }
}

/// A column named by a text argument of a column function, else the
/// argument itself.
fn column_name(e: &Expr) -> String {
    match e {
        Expr::Str(s) => escape(s),
        other => render(other).text,
    }
}

fn render(e: &Expr) -> Piece {
    match e {
        Expr::Number(n) => number(n),
        Expr::Ident(name) => match name.as_str() {
            "true" | "false" => piece(name.clone(), Kind::Atom),
            _ => piece(escape(name), Kind::Atom),
        },
        Expr::Str(s) => piece(format!("“{}”", escape(s)), Kind::Atom),
        Expr::Unary(op, operand) => {
            let inner = render(operand);
            if *op == "!" {
                let wrap = inner.kind < Kind::Prefix;
                piece(format!("not {}", paren(inner, wrap)), Kind::Prefix)
            } else {
                let wrap = inner.kind < Kind::Power;
                piece(format!("−{}", paren(inner, wrap)), Kind::Prefix)
            }
        }
        Expr::Binary(op, a, b) => binary(op, a, b),
        Expr::Call(name, args) => call(name, args),
    }
}

fn binary(op: &str, a: &Expr, b: &Expr) -> Piece {
    let (l, r) = (render(a), render(b));
    match op {
        "/" => piece(format!("\\frac{{{}}}{{{}}}", l.text, r.text), Kind::Fraction),
        "**" => power(l, &r.text),
        "*" | "%" => {
            let wl = l.kind < Kind::Product;
            // A product on the right is regrouped by the parentheses.
            let wr = r.kind <= Kind::Prefix;
            let sign = if op == "*" { " \\cdot " } else { " mod " };
            piece(format!("{}{sign}{}", paren(l, wl), paren(r, wr)), Kind::Product)
        }
        "+" | "-" => {
            let wl = l.kind < Kind::Sum;
            let wr = r.kind <= Kind::Sum || r.kind == Kind::Prefix;
            let sign = if op == "+" { " + " } else { " − " };
            piece(format!("{}{sign}{}", paren(l, wl), paren(r, wr)), Kind::Sum)
        }
        "<" | "<=" | ">" | ">=" | "==" | "!=" => {
            let kind = if matches!(op, "==" | "!=") { Kind::Equality } else { Kind::Comparison };
            let sign = match op {
                "<" => " < ",
                "<=" => " \\le ",
                ">" => " > ",
                ">=" => " \\ge ",
                "==" => " = ",
                _ => " \\ne ",
            };
            let (wl, wr) = (l.kind <= kind, r.kind <= kind);
            piece(format!("{}{sign}{}", paren(l, wl), paren(r, wr)), kind)
        }
        _ => {
            let (kind, word) = if op == "&&" { (Kind::And, " and ") } else { (Kind::Or, " or ") };
            let (wl, wr) = (l.kind < kind, r.kind <= kind);
            piece(format!("{}{word}{}", paren(l, wl), paren(r, wr)), kind)
        }
    }
}

/// `base` raised to `exponent` (markup).
fn power(base: Piece, exponent: &str) -> Piece {
    let wrap = base.kind < Kind::Atom;
    piece(format!("{}^{{{exponent}}}", paren(base, wrap)), Kind::Power)
}

/// The arguments as written, separated by commas (columns named by text
/// arguments of column functions shown as their names).
fn arguments(name: &str, args: &[Expr]) -> String {
    let column = COLUMN_FUNCTIONS.contains(&name);
    args.iter()
        .enumerate()
        .map(|(k, a)| if column && k == 0 { column_name(a) } else { render(a).text })
        .collect::<Vec<_>>()
        .join(", ")
}

fn call(name: &str, args: &[Expr]) -> Piece {
    let one = |f: &dyn Fn(String) -> Piece| match args {
        [x] => Some(f(render(x).text)),
        _ => None,
    };
    let column = |k: usize| match args.get(k) {
        Some(Expr::Str(s)) => Some(escape(s)),
        _ => None,
    };
    // A row offset as a subscript: i − k, i + k.
    let shifted = |sign: &str| {
        let name = column(0)?;
        let k = match args {
            [_] => "1".to_owned(),
            [_, k] => render(k).text,
            _ => return None,
        };
        Some(piece(format!("{name}_{{i {sign} {k}}}"), Kind::Atom))
    };
    let special = match name {
        "sqrt" => one(&|x| piece(format!("\\sqrt{{{x}}}"), Kind::Atom)),
        "abs" => one(&|x| piece(format!("|{x}|"), Kind::Atom)),
        "floor" => one(&|x| piece(format!("⌊{x}⌋"), Kind::Atom)),
        "ceil" | "ceiling" => one(&|x| piece(format!("⌈{x}⌉"), Kind::Atom)),
        "exp" => one(&|x| piece(format!("e^{{{x}}}"), Kind::Power)),
        "log10" => one(&|x| piece(format!("log_{{10}}({x})"), Kind::Atom)),
        "log2" => one(&|x| piece(format!("log_{{2}}({x})"), Kind::Atom)),
        "normal_cdf" => one(&|x| piece(format!("Φ({x})"), Kind::Atom)),
        "normal_pdf" => one(&|x| piece(format!("φ({x})"), Kind::Atom)),
        "pow" => match args {
            [x, y] => Some(power(render(x), &render(y).text)),
            _ => None,
        },
        "row" if args.is_empty() => Some(piece("i", Kind::Atom)),
        "n_rows" if args.is_empty() => Some(piece("n", Kind::Atom)),
        "col" => match args {
            [_] => column(0).map(|n| piece(n, Kind::Atom)),
            [_, k] => column(0).map(|n| piece(format!("{n}_{{{}}}", render(k).text), Kind::Atom)),
            _ => None,
        },
        "lag" => shifted("−"),
        "lead" => shifted("+"),
        "col_sum" if args.len() == 1 => column(0).map(|n| piece(format!("\\sum {n}"), Kind::Product)),
        "cum_sum" if args.len() == 1 => {
            column(0).map(|n| piece(format!("\\sum_{{j \\le i}} {n}_{{j}}"), Kind::Product))
        }
        _ => None,
    };
    special.unwrap_or_else(|| piece(format!("{}({})", escape(name), arguments(name, args)), Kind::Atom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_viz::math_text::{MathNode, MathText};

    fn t(f: &str) -> String {
        typeset(f).unwrap_or_else(|| panic!("{f:?} not typeset"))
    }

    /// The typeset formula read back as plain text (fractions as a/b,
    /// scripts after _ and ^).
    fn plain(f: &str) -> String {
        MathText::parse(&t(f)).plain_text()
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(t("a + b * c"), "a + b \\cdot c");
        assert_eq!(t("(a + b) * c"), "(a + b) \\cdot c");
        assert_eq!(t("a - (b - c)"), "a − (b − c)");
        assert_eq!(t("a - b - c"), "a − b − c");
        assert_eq!(t("a * (b * c)"), "a \\cdot (b \\cdot c)");
        assert_eq!(t("a % 3"), "a mod 3");
        assert_eq!(t("x ** 2"), "x^{2}");
        // `**` groups to the right; a power as a base is parenthesized.
        assert_eq!(t("x ** y ** 2"), "x^{y^{2}}");
        assert_eq!(t("(x ** y) ** 2"), "(x^{y})^{2}");
        assert_eq!(t("(a + b) ** 2"), "(a + b)^{2}");
        assert_eq!(t("x ** (n + 1)"), "x^{n + 1}");
    }

    /// Prefix minus binds tighter than `**` in the formula language, so
    /// the parentheses in the view show what is computed.
    #[test]
    fn prefix_minus() {
        assert_eq!(t("-x ** 2"), "(−x)^{2}");
        assert_eq!(t("-(x ** 2)"), "−x^{2}");
        assert_eq!(t("-(a + b)"), "−(a + b)");
        assert_eq!(t("a - -b"), "a − (−b)");
        assert_eq!(t("a * -b"), "a \\cdot (−b)");
        assert_eq!(t("+a"), "a");
        assert_eq!(t("!(a > 1)"), "not (a > 1)");
    }

    #[test]
    fn fractions_drop_parentheses() {
        assert_eq!(t("Weight / (Height / 100) ** 2"), "\\frac{Weight}{(\\frac{Height}{100})^{2}}");
        assert_eq!(t("(a + b) / (c - d)"), "\\frac{a + b}{c − d}");
        assert_eq!(plain("(a + b) / (c - d)"), "(a + b)/(c − d)");
        assert_eq!(t("a / b / c"), "\\frac{\\frac{a}{b}}{c}");
        assert_eq!(t("a / b * c"), "\\frac{a}{b} \\cdot c");
        let nodes = MathText::parse(&t("1 / sqrt(x)")).nodes;
        assert_eq!(
            nodes,
            vec![MathNode::Frac(vec![MathNode::Text("1".into())], vec![MathNode::Root(vec![MathNode::Text("x".into())])])]
        );
    }

    #[test]
    fn functions() {
        assert_eq!(t("sqrt(a ** 2 + b ** 2)"), "\\sqrt{a^{2} + b^{2}}");
        assert_eq!(t("abs(x - m)"), "|x − m|");
        assert_eq!(t("exp(-t / tau)"), "e^{\\frac{−t}{tau}}");
        assert_eq!(t("exp(x) ** 2"), "(e^{x})^{2}");
        assert_eq!(t("pow(x, 3)"), "x^{3}");
        assert_eq!(t("ln(x) + log10(y)"), "ln(x) + log_{10}(y)");
        assert_eq!(t("floor(x) + ceiling(y)"), "⌊x⌋ + ⌈y⌉");
        assert_eq!(t("normal_cdf(z)"), "Φ(z)");
        assert_eq!(t("normal_cdf(x, 0, 1)"), "normal\\_cdf(x, 0, 1)");
        assert_eq!(t("round(x, 2)"), "round(x, 2)");
        assert_eq!(t("missing()"), "missing()");
    }

    #[test]
    fn rows_columns_and_sums() {
        assert_eq!(t("col(\"Body mass\") / 2"), "\\frac{Body mass}{2}");
        assert_eq!(t("col(\"x\", 3)"), "x_{3}");
        assert_eq!(t("x - lag(\"x\")"), "x − x_{i − 1}");
        assert_eq!(t("lead(\"x\", 2)"), "x_{i + 2}");
        assert_eq!(t("x / col_sum(\"x\")"), "\\frac{x}{\\sum x}");
        assert_eq!(plain("cum_sum(\"Sales\")"), "Σ_{j ≤ i} Sales_j");
        assert_eq!(t("col_mean(\"x\", \"g\")"), "col\\_mean(x, “g”)");
        assert_eq!(t("(x - col_mean(\"x\")) / col_sd(\"x\")"), "\\frac{x − col\\_mean(x)}{col\\_sd(x)}");
        assert_eq!(t("row() / n_rows()"), "\\frac{i}{n}");
    }

    #[test]
    fn names_numbers_and_text_are_literal() {
        assert_eq!(t("Body_mass_2 / 2"), "\\frac{Body\\_mass\\_2}{2}");
        assert_eq!(plain("Body_mass_2 / 2"), "(Body_mass_2)/2", "underscores shown, not subscripts");
        assert_eq!(t("1.5e-3 * x"), "1.5 \\times 10^{−3} \\cdot x");
        assert_eq!(t("1e6"), "10^{6}");
        assert_eq!(t("2E-05"), "2 \\times 10^{−5}");
        assert_eq!(t("1_000 + x"), "1000 + x");
        assert_eq!(t("\"a_b\" + name"), "“a\\_b” + name");
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(t("x >= 0 && y != 1"), "x \\ge 0 and y \\ne 1");
        assert_eq!(t("a || b && c"), "a or b and c");
        assert_eq!(t("(a || b) && c"), "(a or b) and c");
        assert_eq!(t("x + 1 <= y * 2"), "x + 1 \\le y \\cdot 2");
        assert_eq!(t("x == 1;"), "x = 1");
    }

    #[test]
    fn statements_and_unknown_syntax_are_not_typeset() {
        for f in [
            "",
            "   ",
            "let y = x * 2; y + 1",
            "if x > 0 { 1 } else { 0 }",
            "switch x { 1 => 2, _ => 3 }",
            "s.contains(\"a\")",
            "x // comment",
            "(a + b",
            "a +",
            "f(a b)",
            "x = 2",
            "[1, 2]",
        ] {
            assert_eq!(typeset(f), None, "{f:?}");
        }
        let deep = format!("{}x{}", "(".repeat(5000), ")".repeat(5000));
        assert_eq!(typeset(&deep), None, "too deep");
    }
}

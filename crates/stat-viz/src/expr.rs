//! Arithmetic expressions in one variable, for function curves drawn over the
//! data (e.g. `2.5 + 0.8*x - 0.01*x^2` or `100 * exp(-x / 4)`).
//!
//! Grammar (usual precedence, `^` right-associative and binding tighter than
//! unary minus, so `-x^2` is `−(x²)`):
//!
//! ```text
//! expr    = term   { ("+" | "-") term }
//! term    = unary  { ("*" | "/") unary }
//! unary   = ("-" | "+") unary | power
//! power   = primary [ "^" unary ]
//! primary = number | "x" | "pi" | "e" | name "(" expr { "," expr } ")" | "(" expr ")"
//! ```
//!
//! Functions: `sqrt abs exp ln log10 log sin cos tan asin acos atan sinh cosh
//! tanh floor ceil round min max pow`; `log(v)` is the natural log and
//! `log(v, b)` the base-`b` log. Numbers accept decimals and exponents
//! (`1.5e-3`). Names are case-insensitive.

use std::fmt;

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A constant.
    Number(f64),
    /// The variable `x`.
    Var,
    /// `−a`.
    Neg(Box<Expr>),
    /// `a op b`.
    Binary(Op, Box<Expr>, Box<Expr>),
    /// A function call.
    Call(Func, Vec<Expr>),
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Power.
    Pow,
}

/// A built-in function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    /// Square root.
    Sqrt,
    /// Absolute value.
    Abs,
    /// Exponential.
    Exp,
    /// Natural log (one argument) or log to a base (two).
    Ln,
    /// Base-10 log.
    Log10,
    /// Sine.
    Sin,
    /// Cosine.
    Cos,
    /// Tangent.
    Tan,
    /// Arcsine.
    Asin,
    /// Arccosine.
    Acos,
    /// Arctangent.
    Atan,
    /// Hyperbolic sine.
    Sinh,
    /// Hyperbolic cosine.
    Cosh,
    /// Hyperbolic tangent.
    Tanh,
    /// Round down.
    Floor,
    /// Round up.
    Ceil,
    /// Round to nearest.
    Round,
    /// Smaller of two.
    Min,
    /// Larger of two.
    Max,
    /// `pow(a, b) = a^b`.
    Pow,
}

impl Func {
    fn by_name(name: &str) -> Option<(Self, std::ops::RangeInclusive<usize>)> {
        Some(match name {
            "sqrt" => (Self::Sqrt, 1..=1),
            "abs" => (Self::Abs, 1..=1),
            "exp" => (Self::Exp, 1..=1),
            "ln" | "log" => (Self::Ln, 1..=2),
            "log10" => (Self::Log10, 1..=1),
            "sin" => (Self::Sin, 1..=1),
            "cos" => (Self::Cos, 1..=1),
            "tan" => (Self::Tan, 1..=1),
            "asin" => (Self::Asin, 1..=1),
            "acos" => (Self::Acos, 1..=1),
            "atan" => (Self::Atan, 1..=1),
            "sinh" => (Self::Sinh, 1..=1),
            "cosh" => (Self::Cosh, 1..=1),
            "tanh" => (Self::Tanh, 1..=1),
            "floor" => (Self::Floor, 1..=1),
            "ceil" => (Self::Ceil, 1..=1),
            "round" => (Self::Round, 1..=1),
            "min" => (Self::Min, 2..=2),
            "max" => (Self::Max, 2..=2),
            "pow" => (Self::Pow, 2..=2),
            _ => return None,
        })
    }
}

/// Why an expression could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub message: String,
    /// Character position of the problem.
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.position + 1)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Name(String),
    Sym(char),
}

fn tokenize(text: &str) -> Result<Vec<(Token, usize)>, ParseError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let s: String = chars[start..i].iter().collect();
            let v: f64 =
                s.parse().map_err(|_| ParseError { message: format!("{s:?} is not a number"), position: start })?;
            out.push((Token::Num(v), start));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push((Token::Name(chars[start..i].iter().collect::<String>().to_lowercase()), start));
        } else if "+-*/^(),".contains(c) {
            out.push((Token::Sym(c), i));
            i += 1;
        } else if c == '×' || c == '·' {
            out.push((Token::Sym('*'), i));
            i += 1;
        } else if c == '−' {
            out.push((Token::Sym('-'), i));
            i += 1;
        } else {
            return Err(ParseError { message: format!("unexpected {c:?}"), position: i });
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.0)
    }

    fn here(&self) -> usize {
        self.tokens.get(self.pos).map_or(self.end, |t| t.1)
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError { message: message.into(), position: self.here() })
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Token::Sym(c)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.term()?;
        loop {
            let op = if self.eat('+') {
                Op::Add
            } else if self.eat('-') {
                Op::Sub
            } else {
                return Ok(left);
            };
            let right = self.term()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat('*') {
                Op::Mul
            } else if self.eat('/') {
                Op::Div
            } else {
                return Ok(left);
            };
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        if self.eat('-') {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, ParseError> {
        let base = self.primary()?;
        if self.eat('^') {
            let exponent = self.unary()?;
            return Ok(Expr::Binary(Op::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let start = self.here();
        match self.peek().cloned() {
            Some(Token::Num(v)) => {
                self.pos += 1;
                Ok(Expr::Number(v))
            }
            Some(Token::Sym('(')) => {
                self.pos += 1;
                let e = self.expr()?;
                if !self.eat(')') {
                    return self.error("missing \")\"");
                }
                Ok(e)
            }
            Some(Token::Name(name)) => {
                self.pos += 1;
                match name.as_str() {
                    "x" => return Ok(Expr::Var),
                    "pi" => return Ok(Expr::Number(std::f64::consts::PI)),
                    "e" => return Ok(Expr::Number(std::f64::consts::E)),
                    _ => {}
                }
                let Some((func, arity)) = Func::by_name(&name) else {
                    return Err(ParseError {
                        message: format!("unknown name {name:?} (the variable is x)"),
                        position: start,
                    });
                };
                if !self.eat('(') {
                    return self.error(format!("{name} needs its argument in parentheses"));
                }
                let mut args = vec![self.expr()?];
                while self.eat(',') {
                    args.push(self.expr()?);
                }
                if !self.eat(')') {
                    return self.error("missing \")\"");
                }
                if !arity.contains(&args.len()) {
                    return Err(ParseError {
                        message: format!("{name} takes {} argument(s), not {}", arity.start(), args.len()),
                        position: start,
                    });
                }
                Ok(Expr::Call(func, args))
            }
            Some(Token::Sym(c)) => self.error(format!("unexpected {c:?}")),
            None => self.error("the expression ends too soon"),
        }
    }
}

impl Expr {
    /// Parse `text`.
    ///
    /// # Errors
    /// A [`ParseError`] naming the problem and where it is.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let tokens = tokenize(text)?;
        let end = text.chars().count();
        let mut p = Parser { tokens, pos: 0, end };
        let e = p.expr()?;
        if p.pos < p.tokens.len() {
            return p.error("unexpected text after the expression");
        }
        Ok(e)
    }

    /// The value at `x` (NaN where undefined, e.g. `ln` of a negative number).
    #[must_use]
    pub fn eval(&self, x: f64) -> f64 {
        match self {
            Self::Number(v) => *v,
            Self::Var => x,
            Self::Neg(a) => -a.eval(x),
            Self::Binary(op, a, b) => {
                let (a, b) = (a.eval(x), b.eval(x));
                match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    Op::Pow => a.powf(b),
                }
            }
            Self::Call(f, args) => {
                let a = args[0].eval(x);
                let b = || args.get(1).map_or(f64::NAN, |e| e.eval(x));
                match f {
                    Func::Sqrt => a.sqrt(),
                    Func::Abs => a.abs(),
                    Func::Exp => a.exp(),
                    Func::Ln if args.len() == 2 => a.ln() / b().ln(),
                    Func::Ln => a.ln(),
                    Func::Log10 => a.log10(),
                    Func::Sin => a.sin(),
                    Func::Cos => a.cos(),
                    Func::Tan => a.tan(),
                    Func::Asin => a.asin(),
                    Func::Acos => a.acos(),
                    Func::Atan => a.atan(),
                    Func::Sinh => a.sinh(),
                    Func::Cosh => a.cosh(),
                    Func::Tanh => a.tanh(),
                    Func::Floor => a.floor(),
                    Func::Ceil => a.ceil(),
                    Func::Round => a.round(),
                    Func::Min => a.min(b()),
                    Func::Max => a.max(b()),
                    Func::Pow => a.powf(b()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, x: f64) -> f64 {
        Expr::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).eval(x)
    }

    #[test]
    fn precedence_and_functions() {
        assert_eq!(at("1 + 2 * 3", 0.0), 7.0);
        assert_eq!(at("(1 + 2) * 3", 0.0), 9.0);
        assert_eq!(at("2 ^ 3 ^ 2", 0.0), 512.0, "right-associative");
        assert_eq!(at("-x^2", 3.0), -9.0);
        assert_eq!(at("2^-1", 0.0), 0.5);
        assert_eq!(at("10 - 4 - 3", 0.0), 3.0);
        assert_eq!(at("12 / 4 / 3", 0.0), 1.0);
        assert_eq!(at("2.5 + 0.8*x - 0.01*x^2", 10.0), 9.5);
        assert!((at("100 * exp(-x / 4)", 4.0) - 100.0 / std::f64::consts::E).abs() < 1e-12);
        assert!((at("log(8, 2)", 0.0) - 3.0).abs() < 1e-12);
        assert_eq!(at("log10(1000)", 0.0), 3.0);
        assert_eq!(at("max(x, 2) + min(x, 2)", 5.0), 7.0);
        assert!((at("sin(pi / 2) + cos(0)", 0.0) - 2.0).abs() < 1e-12);
        assert_eq!(at("1.5e2 + .5", 0.0), 150.5);
        assert_eq!(at("SQRT(X)", 16.0), 4.0, "names are case-insensitive");
        assert_eq!(at("3 × x − 1", 2.0), 5.0, "typographic operators");
        assert!(at("ln(x)", -1.0).is_nan());
    }

    #[test]
    fn errors_name_the_problem() {
        for (text, needle) in [
            ("", "ends too soon"),
            ("1 +", "ends too soon"),
            ("(1 + 2", "missing"),
            ("foo(2)", "unknown name"),
            ("y + 1", "unknown name"),
            ("sin 2", "parentheses"),
            ("min(1)", "takes 2"),
            ("1 2", "after the expression"),
            ("2 $ 3", "unexpected"),
        ] {
            let e = Expr::parse(text).unwrap_err();
            assert!(e.message.contains(needle), "{text:?}: {e}");
        }
        assert_eq!(Expr::parse("1 + )").unwrap_err().position, 4);
    }
}

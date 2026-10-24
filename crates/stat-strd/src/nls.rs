//! The NIST StRD nonlinear least-squares problems: a parser for NIST's
//! `.dat` files and the model expressions written in them.
//!
//! The 27 files (`<Name>.dat`, names in [`NLS_DATASETS`]) are kept in
//! `crates/stat-strd/data/nls/`; `scripts/fetch-nist-nls.sh` downloads them again from
//! <https://www.itl.nist.gov/div898/strd/nls/data/LINKS/DATA/> into
//! `crates/stat-strd/data/nls/`, where the test `tests/nls_certified.rs` finds
//! them. Each file holds the model, two certified starting points, certified
//! estimates with standard deviations, the residual sum of squares, residual
//! standard deviation and degrees of freedom, and the data.
//!
//! # File format
//! Recognised lines (after trimming):
//!
//! * a line containing `Level of Difficulty` — `Lower`, `Average` or `Higher`;
//! * the `Model:` section, up to the line containing `Starting`: the model
//!   equation is the text after its `=`, with the error term `+ e` removed;
//!   its left side is `y` or `log[y]` (natural logarithm of the response);
//! * `b<k> = <start 1> <start 2> <estimate> <standard deviation>`;
//! * `Residual Sum of Squares: <v>`, `Residual Standard Deviation: <v>`,
//!   `Degrees of Freedom: <k>`, `Number of Observations: <k>`;
//! * `Data: y x …` (names only) — every following non-blank line is a data row.
//!
//! # Model expressions
//! [`ModelExpr`] parses the arithmetic NIST uses: `+ - * /`, `**` (or `^`)
//! for powers, parentheses or square brackets, the functions `exp`, `log`
//! (natural), `sqrt`, `sin`, `cos`, `tan` and `arctan`, the constant `pi`,
//! parameters `b1, b2, …` and the predictor names of the data header. Its
//! derivatives with respect to each parameter are formed symbolically
//! ([`ModelExpr::derivative`]), so the fits use exact derivatives.

/// The 27 NIST StRD nonlinear regression datasets, in NIST's order of
/// difficulty (lower, average, higher).
pub const NLS_DATASETS: [&str; 27] = [
    "Misra1a", "Chwirut2", "Chwirut1", "Lanczos3", "Gauss1", "Gauss2", "DanWood", "Misra1b", "Kirby2", "Hahn1",
    "Nelson", "MGH17", "Lanczos1", "Lanczos2", "Gauss3", "Misra1c", "Misra1d", "Roszman1", "ENSO", "MGH09", "Thurber",
    "BoxBOD", "Rat42", "MGH10", "Eckerle4", "Rat43", "Bennett5",
];

/// One parsed NIST nonlinear regression problem.
#[derive(Debug, Clone, PartialEq)]
pub struct NlsProblem {
    /// NIST difficulty rating, when the file states one.
    pub difficulty: Option<crate::Difficulty>,
    /// The model equation's right side as written in the file.
    pub model_text: String,
    /// The parsed model.
    pub model: ModelExpr,
    /// Whether the left side is `log[y]` (the model is for `ln y`).
    pub log_response: bool,
    /// Predictor names (data columns after the response).
    pub predictors: Vec<String>,
    /// Response values.
    pub y: Vec<f64>,
    /// Predictor values, one row per observation.
    pub x: Vec<Vec<f64>>,
    /// The two certified starting points.
    pub starts: [Vec<f64>; 2],
    /// Certified `(estimate, standard deviation)` of each parameter.
    pub certified: Vec<(f64, f64)>,
    /// Certified residual sum of squares.
    pub residual_ss: f64,
    /// Certified residual standard deviation.
    pub residual_sd: f64,
    /// Residual degrees of freedom.
    pub df: usize,
}

/// Parse a NIST StRD nonlinear regression file.
///
/// # Errors
/// A description of the first missing or malformed part.
pub fn parse_nls(text: &str) -> Result<NlsProblem, String> {
    let mut difficulty = None;
    let mut model_lines: Vec<&str> = Vec::new();
    let mut in_model = false;
    let mut params: Vec<(usize, [f64; 4])> = Vec::new();
    let (mut rss, mut rsd, mut df, mut n_obs) = (None, None, None, None);
    let mut names: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<f64>> = Vec::new();
    let last_number = |line: &str| line.split_whitespace().last().and_then(|t| t.parse::<f64>().ok());
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(names) = &names {
            if line.is_empty() {
                continue;
            }
            let row: Option<Vec<f64>> = line.split_whitespace().map(|t| t.parse().ok()).collect();
            let row = row.ok_or_else(|| format!("malformed data line {line:?}"))?;
            if row.len() != names.len() {
                return Err(format!("data line {line:?} has {} values, expected {}", row.len(), names.len()));
            }
            rows.push(row);
            continue;
        }
        if line.contains("Level of Difficulty") {
            difficulty = if line.starts_with("Lower") {
                Some(crate::Difficulty::Lower)
            } else if line.starts_with("Average") {
                Some(crate::Difficulty::Average)
            } else if line.starts_with("Higher") {
                Some(crate::Difficulty::Higher)
            } else {
                None
            };
        }
        if let Some(rest) = line.strip_prefix("Model:") {
            in_model = true;
            model_lines.push(rest);
            continue;
        }
        if in_model {
            if line.contains("Starting") || line.contains("Certified") {
                in_model = false;
            } else {
                model_lines.push(line);
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("Data:") {
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            let is_name = |t: &str| {
                t.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    && t.chars().all(|c| c.is_ascii_alphanumeric())
            };
            if tokens.len() >= 2 && tokens.iter().all(|t| is_name(t)) {
                names = Some(tokens.iter().map(|t| (*t).to_owned()).collect());
            }
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() == 6 && tokens[1] == "=" {
            if let Some(k) = tokens[0].strip_prefix('b').and_then(|k| k.parse::<usize>().ok()) {
                let v: Option<Vec<f64>> = tokens[2..].iter().map(|t| t.parse().ok()).collect();
                let v = v.ok_or_else(|| format!("malformed parameter line {line:?}"))?;
                params.push((k, [v[0], v[1], v[2], v[3]]));
                continue;
            }
        }
        if line.starts_with("Residual Sum of Squares") {
            rss = last_number(line);
        } else if line.starts_with("Residual Standard Deviation") {
            rsd = last_number(line);
        } else if line.starts_with("Degrees of Freedom") {
            df = last_number(line).map(|v| v as usize);
        } else if line.starts_with("Number of Observations") {
            n_obs = last_number(line).map(|v| v as usize);
        }
    }
    let names = names.ok_or("no data header (\"Data:  y  x\") found")?;
    params.sort_by_key(|(k, _)| *k);
    if params.is_empty() || params.iter().enumerate().any(|(i, (k, _))| *k != i + 1) {
        return Err("parameters b1, b2, … are missing or out of sequence".into());
    }
    // Some files define a constant before the equation ("pi = 3.14159…" in
    // Roszman1); the expression parser knows π, so its definition is dropped.
    model_lines.retain(|line| {
        let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        compact.strip_prefix("pi=").is_none_or(|v| v.parse::<f64>().is_err())
    });
    let joined = model_lines.join(" ");
    let eq = joined.find('=').ok_or("no model equation found")?;
    let lhs = joined[..eq].split_whitespace().last().unwrap_or("");
    let log_response = match lhs {
        "y" => false,
        "log[y]" | "ln[y]" | "log(y)" => true,
        other => return Err(format!("unexpected left side of the model {other:?}")),
    };
    let mut rhs = joined[eq + 1..].trim().to_owned();
    // Drop the error term "+ e".
    let compact: String = rhs.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.ends_with("+e") {
        let at = rhs.rfind('+').unwrap_or(rhs.len());
        rhs.truncate(at);
    }
    let rhs = rhs.split_whitespace().collect::<Vec<_>>().join(" ");
    let predictors: Vec<String> = names[1..].to_vec();
    let model = ModelExpr::parse(&rhs, &predictors)?;
    let p = params.len();
    if model.n_parameters() > p {
        return Err(format!("the model uses b{} but only {p} parameters are certified", model.n_parameters()));
    }
    if let Some(n) = n_obs {
        if n != rows.len() {
            return Err(format!("{} data rows, but the file states {n} observations", rows.len()));
        }
    }
    Ok(NlsProblem {
        difficulty,
        model_text: rhs,
        model,
        log_response,
        predictors,
        y: rows.iter().map(|r| r[0]).collect(),
        x: rows.iter().map(|r| r[1..].to_vec()).collect(),
        starts: [params.iter().map(|(_, v)| v[0]).collect(), params.iter().map(|(_, v)| v[1]).collect()],
        certified: params.iter().map(|(_, v)| (v[2], v[3])).collect(),
        residual_ss: rss.ok_or("no residual sum of squares")?,
        residual_sd: rsd.ok_or("no residual standard deviation")?,
        df: df.ok_or("no degrees of freedom")?,
    })
}

/// Functions a model expression can call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Function {
    /// `exp`.
    Exp,
    /// Natural logarithm (`log` or `ln`).
    Ln,
    /// `sqrt`.
    Sqrt,
    /// `sin`.
    Sin,
    /// `cos`.
    Cos,
    /// `tan`.
    Tan,
    /// `arctan` (or `atan`).
    Arctan,
}

impl Function {
    fn apply(self, v: f64) -> f64 {
        match self {
            Self::Exp => v.exp(),
            Self::Ln => v.ln(),
            Self::Sqrt => v.sqrt(),
            Self::Sin => v.sin(),
            Self::Cos => v.cos(),
            Self::Tan => v.tan(),
            Self::Arctan => v.atan(),
        }
    }
}

/// A parsed model expression.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelExpr {
    /// A number.
    Number(f64),
    /// Parameter `b(k+1)`.
    Parameter(usize),
    /// Predictor `k`.
    Predictor(usize),
    /// `−a`.
    Neg(Box<ModelExpr>),
    /// `a + b`.
    Add(Box<ModelExpr>, Box<ModelExpr>),
    /// `a − b`.
    Sub(Box<ModelExpr>, Box<ModelExpr>),
    /// `a · b`.
    Mul(Box<ModelExpr>, Box<ModelExpr>),
    /// `a / b`.
    Div(Box<ModelExpr>, Box<ModelExpr>),
    /// `a ** b`.
    Pow(Box<ModelExpr>, Box<ModelExpr>),
    /// `f(a)`.
    Call(Function, Box<ModelExpr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Ident(String),
    Op(char),
    Pow,
    Open,
    Close,
}

fn tokenize(s: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            // Exponent: E or e followed by a digit or a signed digit.
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let next = chars.get(i + 1).copied();
                let after = chars.get(i + 2).copied();
                if next.is_some_and(|d| d.is_ascii_digit())
                    || (matches!(next, Some('+' | '-')) && after.is_some_and(|d| d.is_ascii_digit()))
                {
                    i += 2;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            out.push(Token::Num(text.parse().map_err(|_| format!("bad number {text:?}"))?));
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Ident(chars[start..i].iter().collect()));
        } else if c == '*' && chars.get(i + 1) == Some(&'*') {
            out.push(Token::Pow);
            i += 2;
        } else if c == '^' {
            out.push(Token::Pow);
            i += 1;
        } else if matches!(c, '+' | '-' | '*' | '/') {
            out.push(Token::Op(c));
            i += 1;
        } else if matches!(c, '(' | '[') {
            out.push(Token::Open);
            i += 1;
        } else if matches!(c, ')' | ']') {
            out.push(Token::Close);
            i += 1;
        } else {
            return Err(format!("unexpected character {c:?} in the model"));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    predictors: &'a [String],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }
    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }
    fn expr(&mut self) -> Result<ModelExpr, String> {
        let mut left = self.term()?;
        while let Some(Token::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let right = self.term()?;
            left = if c == '+' {
                ModelExpr::Add(left.into(), right.into())
            } else {
                ModelExpr::Sub(left.into(), right.into())
            };
        }
        Ok(left)
    }
    fn term(&mut self) -> Result<ModelExpr, String> {
        let mut left = self.unary()?;
        while let Some(Token::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.at += 1;
            let right = self.unary()?;
            left = if c == '*' {
                ModelExpr::Mul(left.into(), right.into())
            } else {
                ModelExpr::Div(left.into(), right.into())
            };
        }
        Ok(left)
    }
    fn unary(&mut self) -> Result<ModelExpr, String> {
        match self.peek() {
            Some(Token::Op('-')) => {
                self.at += 1;
                Ok(ModelExpr::Neg(self.unary()?.into()))
            }
            Some(Token::Op('+')) => {
                self.at += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }
    fn power(&mut self) -> Result<ModelExpr, String> {
        let base = self.atom()?;
        if self.peek() == Some(&Token::Pow) {
            self.at += 1;
            // Right associative; the exponent may carry a sign: x**(-1/b3).
            let exponent = self.unary()?;
            return Ok(ModelExpr::Pow(base.into(), exponent.into()));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<ModelExpr, String> {
        match self.next() {
            Some(Token::Num(v)) => Ok(ModelExpr::Number(v)),
            Some(Token::Open) => {
                let e = self.expr()?;
                if self.next() != Some(Token::Close) {
                    return Err("unbalanced brackets in the model".into());
                }
                Ok(e)
            }
            Some(Token::Ident(name)) => {
                let lower = name.to_ascii_lowercase();
                let function = match lower.as_str() {
                    "exp" => Some(Function::Exp),
                    "log" | "ln" => Some(Function::Ln),
                    "sqrt" => Some(Function::Sqrt),
                    "sin" => Some(Function::Sin),
                    "cos" => Some(Function::Cos),
                    "tan" => Some(Function::Tan),
                    "arctan" | "atan" => Some(Function::Arctan),
                    _ => None,
                };
                if let Some(f) = function {
                    if self.peek() != Some(&Token::Open) {
                        return Err(format!("{name} must be followed by a bracket"));
                    }
                    return Ok(ModelExpr::Call(f, self.atom()?.into()));
                }
                if lower == "pi" {
                    return Ok(ModelExpr::Number(std::f64::consts::PI));
                }
                if let Some(k) = lower.strip_prefix('b').and_then(|k| k.parse::<usize>().ok()).filter(|&k| k > 0) {
                    return Ok(ModelExpr::Parameter(k - 1));
                }
                if let Some(k) = self.predictors.iter().position(|p| p.eq_ignore_ascii_case(&name)) {
                    return Ok(ModelExpr::Predictor(k));
                }
                Err(format!("unknown name {name:?} in the model"))
            }
            other => Err(format!("unexpected {other:?} in the model")),
        }
    }
}

impl ModelExpr {
    /// Parse `text` with predictor names `predictors`.
    ///
    /// # Errors
    /// A description of the first syntax error or unknown name.
    pub fn parse(text: &str, predictors: &[String]) -> Result<Self, String> {
        let mut parser = Parser { tokens: tokenize(text)?, at: 0, predictors };
        let e = parser.expr()?;
        if parser.at != parser.tokens.len() {
            return Err(format!("unexpected text after the model: {:?}", &parser.tokens[parser.at..]));
        }
        Ok(e)
    }

    /// Highest parameter index used, plus one.
    #[must_use]
    pub fn n_parameters(&self) -> usize {
        match self {
            Self::Number(_) | Self::Predictor(_) => 0,
            Self::Parameter(k) => k + 1,
            Self::Neg(a) | Self::Call(_, a) => a.n_parameters(),
            Self::Add(a, b) | Self::Sub(a, b) | Self::Mul(a, b) | Self::Div(a, b) | Self::Pow(a, b) => {
                a.n_parameters().max(b.n_parameters())
            }
        }
    }

    /// Value at parameters `b` and predictor values `x`.
    #[must_use]
    pub fn eval(&self, b: &[f64], x: &[f64]) -> f64 {
        match self {
            Self::Number(v) => *v,
            Self::Parameter(k) => b[*k],
            Self::Predictor(k) => x[*k],
            Self::Neg(a) => -a.eval(b, x),
            Self::Add(a, c) => a.eval(b, x) + c.eval(b, x),
            Self::Sub(a, c) => a.eval(b, x) - c.eval(b, x),
            Self::Mul(a, c) => a.eval(b, x) * c.eval(b, x),
            Self::Div(a, c) => a.eval(b, x) / c.eval(b, x),
            Self::Pow(a, c) => {
                let (u, v) = (a.eval(b, x), c.eval(b, x));
                // Whole-number exponents of a negative base are well defined.
                if v.fract() == 0.0 && v.abs() < 64.0 { u.powi(v as i32) } else { u.powf(v) }
            }
            Self::Call(f, a) => f.apply(a.eval(b, x)),
        }
    }

    fn depends_on(&self, k: usize) -> bool {
        match self {
            Self::Number(_) | Self::Predictor(_) => false,
            Self::Parameter(j) => *j == k,
            Self::Neg(a) | Self::Call(_, a) => a.depends_on(k),
            Self::Add(a, b) | Self::Sub(a, b) | Self::Mul(a, b) | Self::Div(a, b) | Self::Pow(a, b) => {
                a.depends_on(k) || b.depends_on(k)
            }
        }
    }

    /// Symbolic derivative with respect to parameter `b(k+1)`.
    #[must_use]
    pub fn derivative(&self, k: usize) -> Self {
        use ModelExpr::{Add, Call, Div, Mul, Neg, Number, Pow, Sub};
        let b = |e: &Self| Box::new(e.clone());
        if !self.depends_on(k) {
            return Number(0.0);
        }
        match self {
            Self::Parameter(_) => Number(1.0),
            Self::Number(_) | Self::Predictor(_) => Number(0.0),
            Neg(a) => Neg(a.derivative(k).into()),
            Add(a, c) => sum(a.derivative(k), c.derivative(k)),
            Sub(a, c) => Sub(a.derivative(k).into(), c.derivative(k).into()),
            Mul(a, c) => sum(product(a.derivative(k), (**c).clone()), product((**a).clone(), c.derivative(k))),
            Div(a, c) => {
                let num =
                    Sub(product(a.derivative(k), (**c).clone()).into(), product((**a).clone(), c.derivative(k)).into());
                Div(num.into(), Pow(b(c), Number(2.0).into()).into())
            }
            Pow(a, c) => {
                if c.depends_on(k) {
                    // d(u^v) = u^v · (v′ ln u + v u′ / u)
                    let term1 = product(c.derivative(k), Call(Function::Ln, b(a)));
                    let term2 = Div(product((**c).clone(), a.derivative(k)).into(), b(a));
                    product(self.clone(), sum(term1, term2))
                } else {
                    // d(u^v) = v u^(v−1) u′
                    let reduced = Pow(b(a), Sub(b(c), Number(1.0).into()).into());
                    product(product((**c).clone(), reduced), a.derivative(k))
                }
            }
            Call(f, a) => {
                let inner = a.derivative(k);
                let outer = match f {
                    Function::Exp => self.clone(),
                    Function::Ln => Div(Number(1.0).into(), b(a)),
                    Function::Sqrt => Div(Number(0.5).into(), b(self)),
                    Function::Sin => Call(Function::Cos, b(a)),
                    Function::Cos => Neg(Call(Function::Sin, b(a)).into()),
                    Function::Tan => {
                        Div(Number(1.0).into(), Pow(Call(Function::Cos, b(a)).into(), Number(2.0).into()).into())
                    }
                    Function::Arctan => {
                        Div(Number(1.0).into(), Add(Number(1.0).into(), Pow(b(a), Number(2.0).into()).into()).into())
                    }
                };
                product(outer, inner)
            }
        }
    }
}

fn product(a: ModelExpr, c: ModelExpr) -> ModelExpr {
    match (&a, &c) {
        (ModelExpr::Number(x), _) if *x == 0.0 => ModelExpr::Number(0.0),
        (_, ModelExpr::Number(x)) if *x == 0.0 => ModelExpr::Number(0.0),
        (ModelExpr::Number(x), _) if *x == 1.0 => c,
        (_, ModelExpr::Number(x)) if *x == 1.0 => a,
        _ => ModelExpr::Mul(a.into(), c.into()),
    }
}

fn sum(a: ModelExpr, c: ModelExpr) -> ModelExpr {
    match (&a, &c) {
        (ModelExpr::Number(x), _) if *x == 0.0 => c,
        (_, ModelExpr::Number(x)) if *x == 0.0 => a,
        _ => ModelExpr::Add(a.into(), c.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["x".to_owned()]
    }

    #[test]
    fn expressions_parse_and_evaluate() {
        let e = ModelExpr::parse("b1*(1-exp[-b2*x])", &names()).unwrap();
        assert!((e.eval(&[2.0, 0.5], &[2.0]) - 2.0 * (1.0 - (-1.0f64).exp())).abs() < 1e-15);
        let e = ModelExpr::parse("b1 * (b2+x)**(-1/b3)", &names()).unwrap();
        assert!((e.eval(&[2.0, 1.0, 4.0], &[15.0]) - 2.0 * 16f64.powf(-0.25)).abs() < 1e-15);
        let e = ModelExpr::parse("b1 - b2*x - arctan[b3/(x-b4)]/pi", &names()).unwrap();
        let want = 1.0 - 2.0 * 3.0 - (0.5f64 / (3.0 - 1.0)).atan() / std::f64::consts::PI;
        assert!((e.eval(&[1.0, 2.0, 0.5, 1.0], &[3.0]) - want).abs() < 1e-15);
        // -x**2 is -(x**2); powers are right associative.
        let e = ModelExpr::parse("-x**2 + 2**3**2 + 1.5E+01", &names()).unwrap();
        assert_eq!(e.eval(&[], &[3.0]), -9.0 + 512.0 + 15.0);
        assert_eq!(e.n_parameters(), 0);
        assert!(ModelExpr::parse("b1*z", &names()).is_err());
        assert!(ModelExpr::parse("(b1*x", &names()).is_err());
    }

    #[test]
    fn symbolic_derivatives_match_differences() {
        let models = [
            "b1*(1-exp[-b2*x])",
            "(b1 + b2*x + b3*x**2 + b4*x**3) / (1 + b5*x + b6*x**2 + b7*x**3)",
            "b1 / ((1+exp[b2-b3*x])**(1/b4))",
            "b1 + b2*cos( 2*pi*x/12 ) + b3*sin( 2*pi*x/b4 ) + sqrt(b5*x) + x**b6 + log(b7*x) + tan(b1/x)",
            "b1*(x**2+x*b2) / (x**2+x*b3+b4)",
        ];
        let b = [1.3, 0.7, 0.4, 2.2, 0.3, 0.6, 1.1];
        for m in models {
            let e = ModelExpr::parse(m, &names()).unwrap();
            for k in 0..e.n_parameters() {
                let d = e.derivative(k);
                for x in [0.5, 1.7, 3.1] {
                    let h = 1e-6 * b[k];
                    let (mut up, mut down) = (b, b);
                    up[k] += h;
                    down[k] -= h;
                    let num = (e.eval(&up, &[x]) - e.eval(&down, &[x])) / (2.0 * h);
                    let exact = d.eval(&b, &[x]);
                    assert!((num - exact).abs() < 1e-6 * (1.0 + exact.abs()), "{m} d/db{}: {num} vs {exact}", k + 1);
                }
            }
        }
    }

    /// A file in NIST's layout with invented data (not a NIST dataset).
    pub(crate) const SYNTHETIC: &str = include_str!("../tests/data/synthetic_nls.dat");

    #[test]
    fn parses_a_file_in_nist_layout() {
        let p = parse_nls(SYNTHETIC).unwrap();
        assert_eq!(p.difficulty, Some(crate::Difficulty::Lower));
        assert_eq!(p.predictors, vec!["x".to_owned()]);
        assert!(!p.log_response);
        assert_eq!(p.starts[0].len(), 2);
        assert_eq!(p.certified.len(), 2);
        assert_eq!(p.y.len(), p.x.len());
        assert_eq!(p.df, p.y.len() - 2);
        assert_eq!(p.model_text, "b1*(1-exp[-b2*x])");
        assert!(parse_nls("no content").is_err());
    }
}

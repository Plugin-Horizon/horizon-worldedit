//! `//calc <expression>`: arithmetic with the usual functions, as far as WorldEdit's expression
//! language goes without variables: `+ - * / % ^ **`, brackets, `pi`, `e`, and the math functions.

use super::*;

impl Engine {
    pub(super) fn calc(&self, args: &Args) -> R {
        if args.words.is_empty() {
            return Err(usage("calc"));
        }
        let text = args.words.join(" ");
        let value = evaluate(&text).map_err(|e| format!("'{text}' could not be parsed as a valid expression: '{e}'"))?;
        // WorldEdit prints the words as a Java list: `[2, +, 2] = 4`.
        Ok(vec![format!("[{}] = {}", args.words.join(", "), format_number(value))])
    }
}

/// `DecimalFormat("#,##0.#####")` in English.
pub fn format_number(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "\u{221e}" } else { "-\u{221e}" }.into();
    }
    let rounded = format!("{:.5}", v.abs());
    let (int, frac) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let frac = frac.trim_end_matches('0');
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let negative = v < 0.0 && (grouped != "0" || !frac.is_empty());
    format!("{}{}{}{}", if negative { "-" } else { "" }, grouped, if frac.is_empty() { "" } else { "." }, frac)
}

struct Parser<'a> {
    chars: Vec<char>,
    at: usize,
    text: &'a str,
}

pub fn evaluate(text: &str) -> Result<f64, String> {
    let mut p = Parser { chars: text.chars().collect(), at: 0, text };
    let v = p.sum()?;
    p.skip();
    if p.at < p.chars.len() {
        return Err(format!("Extra character found at position {}", p.at + 1));
    }
    Ok(v)
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }
    fn eat(&mut self, c: char) -> bool {
        self.skip();
        if self.chars.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        loop {
            if self.eat('+') {
                v += self.product()?;
            } else if self.eat('-') {
                v -= self.product()?;
            } else {
                return Ok(v);
            }
        }
    }
    fn product(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            self.skip();
            if self.chars.get(self.at) == Some(&'*') && self.chars.get(self.at + 1) != Some(&'*') {
                self.at += 1;
                v *= self.unary()?;
            } else if self.eat('/') {
                v /= self.unary()?;
            } else if self.eat('%') {
                let d = self.unary()?;
                v = ((v % d) + d) % d;
            } else {
                return Ok(v);
            }
        }
    }
    fn unary(&mut self) -> Result<f64, String> {
        if self.eat('-') {
            return Ok(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }
    fn power(&mut self) -> Result<f64, String> {
        let base = self.atom()?;
        self.skip();
        let double = self.chars.get(self.at) == Some(&'*') && self.chars.get(self.at + 1) == Some(&'*');
        if double {
            self.at += 2;
            return Ok(base.powf(self.unary()?));
        }
        if self.eat('^') {
            return Ok(base.powf(self.unary()?));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<f64, String> {
        self.skip();
        let start = self.at;
        match self.chars.get(self.at).copied() {
            None => Err("Unexpected end of expression".into()),
            Some('(') => {
                self.at += 1;
                let v = self.sum()?;
                if !self.eat(')') {
                    return Err(format!("Expected ')' at position {}", self.at + 1));
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                while self.chars.get(self.at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                    self.at += 1;
                }
                if matches!(self.chars.get(self.at), Some('e' | 'E')) && self.chars.get(self.at + 1).is_some_and(|c| c.is_ascii_digit() || *c == '-' || *c == '+') {
                    self.at += 2;
                    while self.chars.get(self.at).is_some_and(|c| c.is_ascii_digit()) {
                        self.at += 1;
                    }
                }
                let word: String = self.chars[start..self.at].iter().collect();
                word.parse::<f64>().map_err(|_| format!("Invalid number '{word}'"))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                while self.chars.get(self.at).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
                    self.at += 1;
                }
                let name: String = self.chars[start..self.at].iter().collect::<String>().to_ascii_lowercase();
                if self.eat('(') {
                    let mut args = Vec::new();
                    if !self.eat(')') {
                        loop {
                            args.push(self.sum()?);
                            if self.eat(')') {
                                break;
                            }
                            if !self.eat(',') {
                                return Err(format!("Expected ',' or ')' at position {}", self.at + 1));
                            }
                        }
                    }
                    return call(&name, &args);
                }
                match name.as_str() {
                    "pi" => Ok(std::f64::consts::PI),
                    "e" => Ok(std::f64::consts::E),
                    "true" => Ok(1.0),
                    "false" => Ok(0.0),
                    _ => Err(format!("Variable '{name}' not found")),
                }
            }
            Some(c) => Err(format!("Unexpected '{c}' at position {} in '{}'", self.at + 1, self.text)),
        }
    }
}

fn call(name: &str, a: &[f64]) -> Result<f64, String> {
    let one = |f: fn(f64) -> f64| -> Result<f64, String> {
        match a {
            [x] => Ok(f(*x)),
            _ => Err(format!("Function '{name}' requires 1 argument(s), but {} were given", a.len())),
        }
    };
    let two = |f: fn(f64, f64) -> f64| -> Result<f64, String> {
        match a {
            [x, y] => Ok(f(*x, *y)),
            _ => Err(format!("Function '{name}' requires 2 argument(s), but {} were given", a.len())),
        }
    };
    match name {
        "abs" => one(f64::abs),
        "acos" => one(f64::acos),
        "asin" => one(f64::asin),
        "atan" => one(f64::atan),
        "atan2" => two(f64::atan2),
        "cbrt" => one(f64::cbrt),
        "ceil" => one(f64::ceil),
        "cos" => one(f64::cos),
        "cosh" => one(f64::cosh),
        "deg" | "degrees" => one(f64::to_degrees),
        "exp" => one(f64::exp),
        "floor" => one(f64::floor),
        "hypot" => two(f64::hypot),
        "ln" => one(f64::ln),
        "log" => one(f64::ln),
        "log10" => one(f64::log10),
        "max" => two(f64::max),
        "min" => two(f64::min),
        "pow" => two(f64::powf),
        "rad" | "radians" => one(f64::to_radians),
        "rint" => one(|x| {
            let r = x.round();
            if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 { r - x.signum() } else { r }
        }),
        "round" => one(|x| (x + 0.5).floor()),
        "sin" => one(f64::sin),
        "sinh" => one(f64::sinh),
        "sqrt" => one(f64::sqrt),
        "tan" => one(f64::tan),
        "tanh" => one(f64::tanh),
        "sign" | "signum" => one(|x| if x == 0.0 || x.is_nan() { x } else { x.signum() }),
        _ => Err(format!("Function '{name}' not found")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_format() {
        assert_eq!(evaluate("2+3*4").unwrap(), 14.0);
        assert_eq!(evaluate("(2+3)*4").unwrap(), 20.0);
        assert_eq!(evaluate("2^3^2").unwrap(), 512.0);
        assert_eq!(evaluate("-2^2").unwrap(), -4.0);
        assert_eq!(evaluate("sqrt(16)+max(1,2)").unwrap(), 6.0);
        assert_eq!(evaluate("7 % 3").unwrap(), 1.0);
        assert!(evaluate("2 +").is_err() && evaluate("foo").is_err() && evaluate("1 2").is_err());
        assert_eq!(format_number(1234567.0), "1,234,567");
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(1.0 / 3.0), "0.33333");
        assert_eq!(format_number(-12.5), "-12.5");
        assert_eq!(format_number(f64::NAN), "NaN");
    }
}

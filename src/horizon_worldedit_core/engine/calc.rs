//! `//calc <expression>`: WorldEdit's expression language (`expr.rs`) without variables.

use super::expr::Expr;
use super::*;

impl Engine {
    pub(super) fn calc(&mut self, actor: &Actor, args: &Args) -> R {
        if args.words.is_empty() {
            return Err(usage("calc"));
        }
        let text = args.words.join(" ");
        let expr = Expr::parse(&text).map_err(|e| format!("'{text}' could not be parsed as a valid expression: '{e}'"))?;
        let timeout = self.timeout_of(actor);
        let value = expr.run_in(&mut HashMap::new(), &super::expr::NoEnvPublic, Some(timeout))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate(text: &str) -> Result<f64, String> {
        Expr::parse(text)?.run(&mut HashMap::new())
    }

    #[test]
    fn arithmetic_and_format() {
        assert_eq!(evaluate("2+3*4").unwrap(), 14.0);
        assert_eq!(evaluate("(2+3)*4").unwrap(), 20.0);
        // The grammar's power is left associative and the sign binds tighter.
        assert_eq!(evaluate("2^3^2").unwrap(), 64.0);
        assert_eq!(evaluate("-2^2").unwrap(), 4.0);
        assert_eq!(evaluate("sqrt(16)+max(1,2)").unwrap(), 6.0);
        assert_eq!(evaluate("7 % 3").unwrap(), 1.0);
        assert!(evaluate("2 +").is_err() && evaluate("foo").is_err());
        assert_eq!(format_number(1234567.0), "1,234,567");
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(1.0 / 3.0), "0.33333");
        assert_eq!(format_number(-12.5), "-12.5");
        assert_eq!(format_number(f64::NAN), "NaN");
    }
}

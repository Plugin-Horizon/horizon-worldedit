//! The expression language of `//generate` and `//deform` (a subset of WorldEdit's `Expression`): numbers, variables,
//! `+ - * / % ^ **`, comparisons, `&& || !`, `a ? b : c`, assignments (`= += -= *= /= %= ^=`), several statements
//! separated by `;`, and the math functions of `//calc`. Parsed once, run once per block.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Node {
    Num(f64),
    Var(String),
    Neg(Box<Node>),
    Not(Box<Node>),
    Bin(char, Box<Node>, Box<Node>),
    /// `<`, `>`, `<=`, `>=`, `==`, `!=`, `&&`, `||` by their first letters: l g L G e n a o.
    Cmp(char, Box<Node>, Box<Node>),
    Cond(Box<Node>, Box<Node>, Box<Node>),
    Assign(String, Option<char>, Box<Node>),
    Call(String, Vec<Node>),
    Seq(Vec<Node>),
}

pub struct Expr {
    root: Node,
}

struct P {
    c: Vec<char>,
    at: usize,
}

impl P {
    fn skip(&mut self) {
        while self.c.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }
    fn peek(&mut self) -> Option<char> {
        self.skip();
        self.c.get(self.at).copied()
    }
    fn starts(&mut self, s: &str) -> bool {
        self.skip();
        s.chars().enumerate().all(|(i, ch)| self.c.get(self.at + i) == Some(&ch))
    }
    fn take(&mut self, s: &str) -> bool {
        if self.starts(s) {
            self.at += s.chars().count();
            true
        } else {
            false
        }
    }
    fn err(&self, what: &str) -> String {
        format!("{what} at position {}", self.at + 1)
    }

    fn seq(&mut self) -> Result<Node, String> {
        let mut items = vec![self.assign()?];
        while self.take(";") {
            if self.peek().is_none() {
                break;
            }
            items.push(self.assign()?);
        }
        Ok(if items.len() == 1 { items.pop().unwrap_or(Node::Num(0.0)) } else { Node::Seq(items) })
    }

    fn assign(&mut self) -> Result<Node, String> {
        // `name <op>= value`
        self.skip();
        let save = self.at;
        if self.peek().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
            let name = self.ident();
            for (tok, op) in [("+=", Some('+')), ("-=", Some('-')), ("*=", Some('*')), ("/=", Some('/')), ("%=", Some('%')), ("^=", Some('^'))] {
                if self.take(tok) {
                    return Ok(Node::Assign(name, op, Box::new(self.assign()?)));
                }
            }
            if self.starts("=") && !self.starts("==") {
                self.at += 1;
                return Ok(Node::Assign(name, None, Box::new(self.assign()?)));
            }
        }
        self.at = save;
        self.ternary()
    }

    fn ident(&mut self) -> String {
        self.skip();
        let start = self.at;
        while self.c.get(self.at).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
            self.at += 1;
        }
        self.c[start..self.at].iter().collect::<String>().to_ascii_lowercase()
    }

    fn ternary(&mut self) -> Result<Node, String> {
        let cond = self.or()?;
        if self.take("?") {
            let a = self.assign()?;
            if !self.take(":") {
                return Err(self.err("Expected ':'"));
            }
            let b = self.assign()?;
            return Ok(Node::Cond(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn or(&mut self) -> Result<Node, String> {
        let mut v = self.and()?;
        while self.take("||") {
            v = Node::Cmp('o', Box::new(v), Box::new(self.and()?));
        }
        Ok(v)
    }
    fn and(&mut self) -> Result<Node, String> {
        let mut v = self.cmp()?;
        while self.take("&&") {
            v = Node::Cmp('a', Box::new(v), Box::new(self.cmp()?));
        }
        Ok(v)
    }
    fn cmp(&mut self) -> Result<Node, String> {
        let mut v = self.sum()?;
        loop {
            let op = if self.take("<=") {
                'L'
            } else if self.take(">=") {
                'G'
            } else if self.take("==") {
                'e'
            } else if self.take("!=") {
                'n'
            } else if self.take("<") {
                'l'
            } else if self.take(">") {
                'g'
            } else {
                return Ok(v);
            };
            v = Node::Cmp(op, Box::new(v), Box::new(self.sum()?));
        }
    }
    fn sum(&mut self) -> Result<Node, String> {
        let mut v = self.product()?;
        loop {
            self.skip();
            // `+=` and `-=` belong to the assignment.
            if self.starts("+") && !self.starts("+=") {
                self.at += 1;
                v = Node::Bin('+', Box::new(v), Box::new(self.product()?));
            } else if self.starts("-") && !self.starts("-=") {
                self.at += 1;
                v = Node::Bin('-', Box::new(v), Box::new(self.product()?));
            } else {
                return Ok(v);
            }
        }
    }
    fn product(&mut self) -> Result<Node, String> {
        let mut v = self.unary()?;
        loop {
            self.skip();
            if self.starts("*") && !self.starts("**") && !self.starts("*=") {
                self.at += 1;
                v = Node::Bin('*', Box::new(v), Box::new(self.unary()?));
            } else if self.starts("/") && !self.starts("/=") {
                self.at += 1;
                v = Node::Bin('/', Box::new(v), Box::new(self.unary()?));
            } else if self.starts("%") && !self.starts("%=") {
                self.at += 1;
                v = Node::Bin('%', Box::new(v), Box::new(self.unary()?));
            } else {
                return Ok(v);
            }
        }
    }
    fn unary(&mut self) -> Result<Node, String> {
        if self.starts("-") && !self.starts("-=") {
            self.at += 1;
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        if self.starts("+") && !self.starts("+=") {
            self.at += 1;
            return self.unary();
        }
        if self.starts("!") && !self.starts("!=") {
            self.at += 1;
            return Ok(Node::Not(Box::new(self.unary()?)));
        }
        self.power()
    }
    fn power(&mut self) -> Result<Node, String> {
        let base = self.atom()?;
        if self.starts("**") {
            self.at += 2;
            return Ok(Node::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        if self.starts("^") && !self.starts("^=") {
            self.at += 1;
            return Ok(Node::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<Node, String> {
        match self.peek() {
            None => Err("Unexpected end of expression".into()),
            Some('(') => {
                self.at += 1;
                let v = self.seq()?;
                if !self.take(")") {
                    return Err(self.err("Expected ')'"));
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.at;
                while self.c.get(self.at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                    self.at += 1;
                }
                if matches!(self.c.get(self.at), Some('e' | 'E')) && self.c.get(self.at + 1).is_some_and(|c| c.is_ascii_digit() || *c == '-' || *c == '+') {
                    self.at += 2;
                    while self.c.get(self.at).is_some_and(|c| c.is_ascii_digit()) {
                        self.at += 1;
                    }
                }
                let word: String = self.c[start..self.at].iter().collect();
                word.parse::<f64>().map(Node::Num).map_err(|_| format!("Invalid number '{word}'"))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let name = self.ident();
                if self.take("(") {
                    let mut args = Vec::new();
                    if !self.take(")") {
                        loop {
                            args.push(self.assign()?);
                            if self.take(")") {
                                break;
                            }
                            if !self.take(",") {
                                return Err(self.err("Expected ',' or ')'"));
                            }
                        }
                    }
                    return Ok(Node::Call(name, args));
                }
                Ok(Node::Var(name))
            }
            Some(c) => Err(self.err(&format!("Unexpected '{c}'"))),
        }
    }
}

/// A deterministic stand-in for `rand`: the same input always gives the same value in `[0, 1)`.
fn unit_hash(x: f64) -> f64 {
    let mut h = x.to_bits() ^ 0x9E37_79B9_7F4A_7C15;
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

impl Expr {
    pub fn parse(text: &str) -> Result<Expr, String> {
        let mut p = P { c: text.chars().collect(), at: 0 };
        let root = p.seq()?;
        if p.peek().is_some() {
            return Err(p.err("Extra character found"));
        }
        Ok(Expr { root })
    }

    pub fn run(&self, vars: &mut HashMap<String, f64>) -> Result<f64, String> {
        eval(&self.root, vars)
    }

    /// Every variable the expression assigns or reads that is not a built-in constant, for early errors.
    pub fn unknown_variables(&self, known: &[&str]) -> Vec<String> {
        let mut assigned: Vec<String> = known.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        fn walk(n: &Node, assigned: &mut Vec<String>, out: &mut Vec<String>) {
            match n {
                Node::Num(_) => {}
                Node::Var(v) => {
                    if !matches!(v.as_str(), "pi" | "e" | "true" | "false") && !assigned.contains(v) && !out.contains(v) {
                        out.push(v.clone());
                    }
                }
                Node::Neg(a) | Node::Not(a) => walk(a, assigned, out),
                Node::Bin(_, a, b) | Node::Cmp(_, a, b) => {
                    walk(a, assigned, out);
                    walk(b, assigned, out);
                }
                Node::Cond(a, b, c) => {
                    walk(a, assigned, out);
                    walk(b, assigned, out);
                    walk(c, assigned, out);
                }
                Node::Assign(name, op, v) => {
                    if op.is_some() && !assigned.contains(name) && !out.contains(name) {
                        out.push(name.clone());
                    }
                    walk(v, assigned, out);
                    assigned.push(name.clone());
                }
                Node::Call(_, args) => args.iter().for_each(|a| walk(a, assigned, out)),
                Node::Seq(items) => items.iter().for_each(|a| walk(a, assigned, out)),
            }
        }
        walk(&self.root, &mut assigned, &mut out);
        out
    }
}

fn eval(n: &Node, vars: &mut HashMap<String, f64>) -> Result<f64, String> {
    Ok(match n {
        Node::Num(v) => *v,
        Node::Var(name) => match name.as_str() {
            "pi" => std::f64::consts::PI,
            "e" => std::f64::consts::E,
            "true" => 1.0,
            "false" => 0.0,
            _ => *vars.get(name).ok_or_else(|| format!("Variable '{name}' not found"))?,
        },
        Node::Neg(a) => -eval(a, vars)?,
        Node::Not(a) => f64::from(eval(a, vars)? <= 0.0),
        Node::Bin(op, a, b) => {
            let (x, y) = (eval(a, vars)?, eval(b, vars)?);
            match op {
                '+' => x + y,
                '-' => x - y,
                '*' => x * y,
                '/' => x / y,
                '%' => ((x % y) + y) % y,
                _ => x.powf(y),
            }
        }
        Node::Cmp(op, a, b) => {
            let x = eval(a, vars)?;
            match op {
                'a' => {
                    if x <= 0.0 {
                        0.0
                    } else {
                        f64::from(eval(b, vars)? > 0.0)
                    }
                }
                'o' => {
                    if x > 0.0 {
                        1.0
                    } else {
                        f64::from(eval(b, vars)? > 0.0)
                    }
                }
                _ => {
                    let y = eval(b, vars)?;
                    f64::from(match op {
                        'l' => x < y,
                        'g' => x > y,
                        'L' => x <= y,
                        'G' => x >= y,
                        'e' => (x - y).abs() < 1e-9 * x.abs().max(y.abs()).max(1.0),
                        _ => (x - y).abs() >= 1e-9 * x.abs().max(y.abs()).max(1.0),
                    })
                }
            }
        }
        Node::Cond(c, a, b) => {
            if eval(c, vars)? > 0.0 {
                eval(a, vars)?
            } else {
                eval(b, vars)?
            }
        }
        Node::Assign(name, op, v) => {
            let value = eval(v, vars)?;
            let new = match op {
                None => value,
                Some(op) => {
                    let old = *vars.get(name).ok_or_else(|| format!("Variable '{name}' not found"))?;
                    match op {
                        '+' => old + value,
                        '-' => old - value,
                        '*' => old * value,
                        '/' => old / value,
                        '%' => ((old % value) + value) % value,
                        _ => old.powf(value),
                    }
                }
            };
            vars.insert(name.clone(), new);
            new
        }
        Node::Call(name, args) => {
            let mut values = Vec::with_capacity(args.len());
            // `if(c, a, b)` only runs the branch it takes.
            if name == "if" && args.len() == 3 {
                return if eval(&args[0], vars)? > 0.0 { eval(&args[1], vars) } else { eval(&args[2], vars) };
            }
            for a in args {
                values.push(eval(a, vars)?);
            }
            match (name.as_str(), values.as_slice()) {
                ("clamp", [x, lo, hi]) => x.clamp(*lo, *hi),
                ("rand" | "random", [seed]) => unit_hash(*seed),
                ("randint", [n]) => (unit_hash(*n) * n.max(1.0)).floor(),
                ("mod", [x, y]) => ((x % y) + y) % y,
                ("fmod", [x, y]) => x % y,
                ("sqr" | "sq", [x]) => x * x,
                ("lerp", [a, b, t]) => a + (b - a) * t,
                _ => super::calc::call(name, &values)?,
            }
        }
        Node::Seq(items) => {
            let mut last = 0.0;
            for i in items {
                last = eval(i, vars)?;
            }
            last
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, vars: &[(&str, f64)]) -> f64 {
        let mut m: HashMap<String, f64> = vars.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        Expr::parse(text).unwrap().run(&mut m).unwrap()
    }

    #[test]
    fn arithmetic_comparisons_and_variables() {
        assert_eq!(run("2+3*4", &[]), 14.0);
        assert_eq!(run("x*x + y*y < 1", &[("x", 0.5), ("y", 0.5)]), 1.0);
        assert_eq!(run("x*x + y*y < 1", &[("x", 1.0), ("y", 1.0)]), 0.0);
        assert_eq!(run("a = 3; a += 2; a * 2", &[]), 10.0);
        assert_eq!(run("x > 0 ? 7 : 9", &[("x", -1.0)]), 9.0);
        assert_eq!(run("if(x, 1, 2)", &[("x", 0.0)]), 2.0);
        assert_eq!(run("!0 && 1 || 0", &[]), 1.0);
        assert_eq!(run("sqrt(x^2 + 0) == 3", &[("x", 3.0)]), 1.0);
        assert_eq!(run("clamp(5, 0, 2)", &[]), 2.0);
    }

    #[test]
    fn assignments_change_the_variables() {
        let mut m: HashMap<String, f64> = HashMap::new();
        m.insert("x".into(), 1.0);
        m.insert("y".into(), 2.0);
        Expr::parse("y -= 0.5; x = x + y").unwrap().run(&mut m).unwrap();
        assert_eq!((m["x"], m["y"]), (2.5, 1.5));
    }

    #[test]
    fn errors_name_the_problem() {
        assert!(Expr::parse("2 +").is_err());
        assert!(Expr::parse("1 2").is_err());
        let mut m = HashMap::new();
        assert_eq!(Expr::parse("foo + 1").unwrap().run(&mut m).unwrap_err(), "Variable 'foo' not found");
        assert_eq!(Expr::parse("bar(1)").unwrap().run(&mut m).unwrap_err(), "Function 'bar' not found");
        assert_eq!(Expr::parse("foo = 1; foo + bar").unwrap().unknown_variables(&["x"]), vec!["bar".to_string()]);
    }
}

//! WorldEdit's expression language (`Expression.g4`, `CompilingVisitor`, `Functions`): numbers, variables, the operators
//! `+ - * / % ^ ** << >> ~ ! ++ -- == != ~= < <= > >= && || ?:` and the assignments, the statements `if else while do for
//! switch break continue return` and blocks, factorials (`x!`), and the function library (math, `rotate`, `swap`, `megabuf`,
//! `gmegabuf`, `closest`, `gclosest`, `random`, `randint`, `perlin`, `voronoi`, `ridgedmulti`, `query`, `queryAbs`,
//! `queryRel`). Parsed once, run once per block. Identifiers are case sensitive; `e`, `pi`, `true` and `false` are constants.

use super::noise;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Loops stop after this many iterations (`ExpressionHelper.checkIterations`).
const MAX_ITERATIONS: u32 = 256;

#[derive(Clone, Debug)]
enum E {
    Num(f64),
    Id(String, usize),
    Neg(Box<E>),
    Not(Box<E>),
    Complement(Box<E>),
    Factorial(Box<E>),
    Bin(Op, Box<E>, Box<E>),
    And(Box<E>, Box<E>),
    Or(Box<E>, Box<E>),
    Ternary(Box<E>, Box<S>, Box<S>),
    Assign(String, Option<Op>, Box<E>, usize),
    Crement { name: String, delta: f64, post: bool, pos: usize },
    Call(String, Vec<E>, usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Near,
}

#[derive(Clone, Debug)]
enum S {
    Block(Vec<S>),
    If(E, Box<S>, Option<Box<S>>),
    While(E, Box<S>, usize),
    Do(Box<S>, E, usize),
    For { init: E, cond: E, update: E, body: Box<S>, pos: usize },
    Range { counter: String, first: E, last: E, body: Box<S>, pos: usize },
    Break,
    Continue,
    Return(E),
    Switch { target: E, cases: Vec<(f64, Vec<S>)>, default: Option<Vec<S>>, pos: usize },
    Expr(E),
    Empty,
}

/// What a run sees of the world: `query(x, y, z, type, data)` and its absolute and relative forms read the legacy id and data
/// of a block. `mode` is 0 for the expression's own coordinates, 1 absolute, 2 relative to the block being computed.
pub trait ExprEnv {
    fn query(&self, mode: u8, x: f64, y: f64, z: f64) -> (f64, f64);
}

/// An environment that knows no block (`query` answers -1): for expressions that read no world.
pub struct NoEnvPublic;

impl ExprEnv for NoEnvPublic {
    fn query(&self, _mode: u8, _x: f64, _y: f64, _z: f64) -> (f64, f64) {
        (-1.0, -1.0)
    }
}

type Buffer = HashMap<i32, Box<[f64; 1024]>>;

pub struct Expr {
    program: Vec<S>,
    /// The megabuffer of this expression (`megabuf`, `closest`).
    megabuf: Mutex<Buffer>,
}

impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expr({} statements)", self.program.len())
    }
}

/// The megabuffer all expressions share (`gmegabuf`, `gclosest`).
static GLOBAL_MEGABUF: Mutex<Option<Buffer>> = Mutex::new(None);

// ---- lexer ----

#[derive(Clone, Debug, PartialEq)]
enum T {
    Num(f64),
    Id(String),
    P(&'static str),
    End,
}

/// The tokens that are not words or numbers, the longer ones first.
const PUNCT: &[&str] = &[
    "<<", ">>", "+=", "-=", "*=", "/=", "%=", "^=", "==", "!=", "~=", "<=", ">=", "&&", "||", "++", "--", "**", "+", "-", "*", "/", "%", "^", "=", "~", "<", ">", ",", "(", ")", "{", "}", ";", "?", ":", "!",
];

fn lex(text: &str) -> Result<Vec<(T, usize)>, String> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let mut s = String::new();
            while c.get(i).is_some_and(char::is_ascii_digit) {
                s.push(c[i]);
                i += 1;
            }
            if c.get(i) == Some(&'.') && c.get(i + 1).is_some_and(char::is_ascii_digit) {
                s.push('.');
                i += 1;
                while c.get(i).is_some_and(char::is_ascii_digit) {
                    s.push(c[i]);
                    i += 1;
                }
            }
            if matches!(c.get(i), Some('e' | 'E')) {
                let mut j = i + 1;
                if matches!(c.get(j), Some('+' | '-')) {
                    j += 1;
                }
                if c.get(j).is_some_and(char::is_ascii_digit) {
                    s.extend(&c[i..j]);
                    i = j;
                    while c.get(i).is_some_and(char::is_ascii_digit) {
                        s.push(c[i]);
                        i += 1;
                    }
                }
            }
            let v = s.parse::<f64>().map_err(|_| format!("Lexer error at {}: bad number", start + 1))?;
            out.push((T::Num(v), start));
            continue;
        }
        if ch.is_ascii_alphabetic() {
            let mut s = String::new();
            while c.get(i).is_some_and(|x| x.is_ascii_alphanumeric() || *x == '_') {
                s.push(c[i]);
                i += 1;
            }
            out.push((T::Id(s), start));
            continue;
        }
        let rest: String = c[i..c.len().min(i + 2)].iter().collect();
        match PUNCT.iter().find(|p| rest.starts_with(**p)) {
            Some(p) => {
                out.push((T::P(p), start));
                i += p.chars().count();
            }
            None => return Err(format!("Lexer error at {}: token recognition error at: '{ch}'", start + 1)),
        }
    }
    out.push((T::End, c.len()));
    Ok(out)
}

// ---- parser ----

struct P {
    t: Vec<(T, usize)>,
    at: usize,
}

type PR<X> = Result<X, String>;

fn perr(pos: usize, what: &str) -> String {
    format!("Parser error at {}: {what}", pos + 1)
}

impl P {
    fn peek(&self) -> &T {
        &self.t[self.at].0
    }
    fn pos(&self) -> usize {
        self.t[self.at].1
    }
    fn next_is_punct(&self, p: &str) -> bool {
        matches!(self.t.get(self.at + 1), Some((T::P(q), _)) if *q == p)
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), T::P(q) if *q == p)
    }
    fn is_id(&self, name: &str) -> bool {
        matches!(self.peek(), T::Id(n) if n == name)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn eat_id(&mut self, name: &str) -> bool {
        if self.is_id(name) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: &str) -> PR<()> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(perr(self.pos(), &format!("mismatched input {}, expecting '{p}'", self.describe())))
        }
    }
    fn describe(&self) -> String {
        match self.peek() {
            T::Num(v) => format!("'{v}'"),
            T::Id(s) => format!("'{s}'"),
            T::P(p) => format!("'{p}'"),
            T::End => "'<EOF>'".to_owned(),
        }
    }
    fn keyword(&self) -> bool {
        matches!(self.peek(), T::Id(n) if matches!(n.as_str(), "if" | "else" | "while" | "do" | "for" | "break" | "continue" | "return" | "switch" | "case" | "default"))
    }

    /// Statements up to `close` (a `}`, or for a case body the next label or `}`), or up to the end of the input.
    fn statements_until(&mut self, close: Option<&str>) -> PR<Vec<S>> {
        let mut out = Vec::new();
        loop {
            let done = match close {
                Some(":") => self.is_id("case") || self.is_id("default") || self.is("}"),
                Some(c) => self.is(c),
                None => false,
            };
            if done || matches!(self.peek(), T::End) {
                break;
            }
            out.push(self.statement()?);
        }
        if out.is_empty() {
            return Err(perr(self.pos(), &format!("mismatched input {}, expecting a statement", self.describe())));
        }
        Ok(out)
    }

    fn statement(&mut self) -> PR<S> {
        let pos = self.pos();
        let s = if self.is("{") {
            self.at += 1;
            let body = self.statements_until(Some("}"))?;
            self.expect("}")?;
            S::Block(body)
        } else if self.eat_id("if") {
            self.expect("(")?;
            let cond = self.expression()?;
            self.expect(")")?;
            let yes = Box::new(self.statement()?);
            let no = if self.eat_id("else") { Some(Box::new(self.statement()?)) } else { None };
            S::If(cond, yes, no)
        } else if self.eat_id("while") {
            self.expect("(")?;
            let cond = self.expression()?;
            self.expect(")")?;
            S::While(cond, Box::new(self.statement()?), pos)
        } else if self.eat_id("do") {
            let body = Box::new(self.statement()?);
            if !self.eat_id("while") {
                return Err(perr(self.pos(), &format!("mismatched input {}, expecting 'while'", self.describe())));
            }
            self.expect("(")?;
            let cond = self.expression()?;
            self.expect(")")?;
            S::Do(body, cond, pos)
        } else if self.eat_id("for") {
            self.expect("(")?;
            let init = self.expression()?;
            if self.eat(",") {
                // `for (counter = first, last)`.
                let E::Assign(counter, None, first, _) = init else {
                    return Err(perr(self.pos(), "mismatched input ',', expecting ';'"));
                };
                let last = self.expression()?;
                self.expect(")")?;
                S::Range { counter, first: *first, last, body: Box::new(self.statement()?), pos }
            } else {
                self.expect(";")?;
                let cond = self.expression()?;
                self.expect(";")?;
                let update = self.expression()?;
                self.expect(")")?;
                S::For { init, cond, update, body: Box::new(self.statement()?), pos }
            }
        } else if self.eat_id("break") {
            S::Break
        } else if self.eat_id("continue") {
            S::Continue
        } else if self.eat_id("return") {
            S::Return(self.expression()?)
        } else if self.eat_id("switch") {
            self.expect("(")?;
            let target = self.expression()?;
            self.expect(")")?;
            self.expect("{")?;
            let mut cases: Vec<(f64, Vec<S>)> = Vec::new();
            let mut default = None;
            loop {
                if self.eat_id("case") {
                    let T::Num(v) = *self.peek() else { return Err(perr(self.pos(), &format!("mismatched input {}, expecting a number", self.describe()))) };
                    self.at += 1;
                    self.expect(":")?;
                    let body = self.statements_until(Some(":"))?;
                    if cases.iter().any(|(k, _)| *k == v) {
                        return Err(format!("Evaluation error at {}: Duplicate cases detected.", pos + 1));
                    }
                    cases.push((v, body));
                } else if self.eat_id("default") {
                    self.expect(":")?;
                    let body = self.statements_until(Some(":"))?;
                    if default.is_some() {
                        return Err(format!("Evaluation error at {}: Duplicate default cases detected.", pos + 1));
                    }
                    default = Some(body);
                } else {
                    break;
                }
            }
            if cases.is_empty() && default.is_none() {
                return Err(perr(self.pos(), &format!("mismatched input {}, expecting 'case'", self.describe())));
            }
            self.expect("}")?;
            S::Switch { target, cases, default, pos }
        } else if self.is(";") {
            self.at += 1;
            return Ok(S::Empty);
        } else {
            if self.keyword() {
                return Err(perr(pos, &format!("mismatched input {}, expecting an expression", self.describe())));
            }
            S::Expr(self.expression()?)
        };
        self.eat(";");
        Ok(s)
    }

    fn expression(&mut self) -> PR<E> {
        // `ID assignmentOperator expression`
        if let T::Id(name) = self.peek().clone() {
            let pos = self.pos();
            if let Some((T::P(p), _)) = self.t.get(self.at + 1) {
                let op = match *p {
                    "=" => Some(None),
                    "+=" => Some(Some(Op::Add)),
                    "-=" => Some(Some(Op::Sub)),
                    "*=" => Some(Some(Op::Mul)),
                    "/=" => Some(Some(Op::Div)),
                    "%=" => Some(Some(Op::Rem)),
                    "^=" => Some(Some(Op::Pow)),
                    _ => None,
                };
                if let Some(op) = op {
                    if !self.keyword() {
                        self.at += 2;
                        return Ok(E::Assign(name, op, Box::new(self.expression()?), pos));
                    }
                }
            }
        }
        self.conditional()
    }

    fn conditional(&mut self) -> PR<E> {
        let cond = self.or()?;
        if self.eat("?") {
            let yes = self.expression()?;
            self.expect(":")?;
            let no = self.conditional()?;
            return Ok(E::Ternary(Box::new(cond), Box::new(S::Expr(yes)), Box::new(S::Expr(no))));
        }
        Ok(cond)
    }

    fn or(&mut self) -> PR<E> {
        let mut v = self.and()?;
        while self.eat("||") {
            v = E::Or(Box::new(v), Box::new(self.and()?));
        }
        Ok(v)
    }

    fn and(&mut self) -> PR<E> {
        let mut v = self.equality()?;
        while self.eat("&&") {
            v = E::And(Box::new(v), Box::new(self.equality()?));
        }
        Ok(v)
    }

    fn equality(&mut self) -> PR<E> {
        let mut v = self.relational()?;
        loop {
            let op = if self.eat("==") {
                Op::Eq
            } else if self.eat("!=") {
                Op::Ne
            } else if self.eat("~=") {
                Op::Near
            } else {
                return Ok(v);
            };
            v = E::Bin(op, Box::new(v), Box::new(self.relational()?));
        }
    }

    fn relational(&mut self) -> PR<E> {
        let mut v = self.shift()?;
        loop {
            let op = if self.eat("<=") {
                Op::Le
            } else if self.eat(">=") {
                Op::Ge
            } else if self.eat("<") {
                Op::Lt
            } else if self.eat(">") {
                Op::Gt
            } else {
                return Ok(v);
            };
            v = E::Bin(op, Box::new(v), Box::new(self.shift()?));
        }
    }

    fn shift(&mut self) -> PR<E> {
        let mut v = self.additive()?;
        loop {
            let op = if self.eat("<<") {
                Op::Shl
            } else if self.eat(">>") {
                Op::Shr
            } else {
                return Ok(v);
            };
            v = E::Bin(op, Box::new(v), Box::new(self.additive()?));
        }
    }

    fn additive(&mut self) -> PR<E> {
        let mut v = self.multiplicative()?;
        loop {
            let op = if self.eat("+") {
                Op::Add
            } else if self.eat("-") {
                Op::Sub
            } else {
                return Ok(v);
            };
            v = E::Bin(op, Box::new(v), Box::new(self.multiplicative()?));
        }
    }

    fn multiplicative(&mut self) -> PR<E> {
        let mut v = self.power()?;
        loop {
            let op = if self.eat("*") {
                Op::Mul
            } else if self.eat("/") {
                Op::Div
            } else if self.eat("%") {
                Op::Rem
            } else {
                return Ok(v);
            };
            v = E::Bin(op, Box::new(v), Box::new(self.power()?));
        }
    }

    /// `left=powerExpression POWER right=unaryExpression`: left associative.
    fn power(&mut self) -> PR<E> {
        let mut v = self.unary()?;
        while self.eat("^") || self.eat("**") {
            v = E::Bin(Op::Pow, Box::new(v), Box::new(self.unary()?));
        }
        Ok(v)
    }

    fn unary(&mut self) -> PR<E> {
        let pos = self.pos();
        for (tok, delta) in [("++", 1.0), ("--", -1.0)] {
            if self.eat(tok) {
                let T::Id(name) = self.peek().clone() else { return Err(perr(self.pos(), &format!("mismatched input {}, expecting an identifier", self.describe()))) };
                self.at += 1;
                return Ok(E::Crement { name, delta, post: false, pos });
            }
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat("-") {
            return Ok(E::Neg(Box::new(self.unary()?)));
        }
        if self.eat("~") {
            return Ok(E::Complement(Box::new(self.unary()?)));
        }
        if self.eat("!") {
            return Ok(E::Not(Box::new(self.unary()?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PR<E> {
        let pos = self.pos();
        // `ID ++` and `ID --`.
        if let T::Id(name) = self.peek().clone() {
            for (tok, delta) in [("++", 1.0), ("--", -1.0)] {
                if self.next_is_punct(tok) && !self.keyword() {
                    self.at += 2;
                    return Ok(E::Crement { name, delta, post: true, pos });
                }
            }
        }
        let mut v = self.primary()?;
        while self.eat("!") {
            v = E::Factorial(Box::new(v));
        }
        Ok(v)
    }

    fn primary(&mut self) -> PR<E> {
        let pos = self.pos();
        match self.peek().clone() {
            T::Num(v) => {
                self.at += 1;
                Ok(E::Num(v))
            }
            T::Id(name) => {
                if self.keyword() {
                    return Err(perr(pos, &format!("mismatched input {}, expecting an expression", self.describe())));
                }
                self.at += 1;
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.is(")") {
                        loop {
                            args.push(self.expression()?);
                            if !self.eat(",") {
                                break;
                            }
                        }
                    }
                    self.expect(")")?;
                    Ok(E::Call(name, args, pos))
                } else {
                    Ok(E::Id(name, pos))
                }
            }
            T::P("(") => {
                self.at += 1;
                let v = self.expression()?;
                self.expect(")")?;
                Ok(v)
            }
            _ => Err(perr(pos, &format!("mismatched input {}, expecting an expression", self.describe()))),
        }
    }
}

// ---- validation ----

/// The parameter counts a function takes (`None`: not a function; an empty list: any number).
fn function_arity(name: &str) -> Option<&'static [usize]> {
    Some(match name {
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "sqrt" | "cbrt" | "abs" | "ceil" | "floor" | "rint" | "exp" | "log" | "log10" | "ln" | "round" | "randint" => &[1],
        "atan2" | "swap" => &[2],
        "rotate" => &[3],
        "min" | "max" => &[],
        "megabuf" | "gmegabuf" => &[1, 2],
        "closest" | "gclosest" => &[6],
        "random" => &[0],
        "perlin" => &[7],
        "voronoi" => &[5],
        "ridgedmulti" => &[6],
        "query" | "queryAbs" | "queryRel" => &[5],
        _ => return None,
    })
}

/// The parameters that must be variables.
fn variable_params(name: &str) -> &'static [usize] {
    match name {
        "rotate" | "swap" => &[0, 1],
        _ => &[],
    }
}

struct Validator {
    bound: Vec<String>,
    problem: Option<String>,
}

impl Validator {
    fn new(known: &[&str]) -> Self {
        Self { bound: known.iter().map(|s| (*s).to_owned()).chain(["e", "pi", "true", "false"].map(str::to_owned)).collect(), problem: None }
    }
    fn fail(&mut self, pos: usize, msg: String) {
        if self.problem.is_none() {
            self.problem = Some(format!("Evaluation error at {}: {msg}", pos + 1));
        }
    }
    fn bind(&mut self, name: &str) {
        if !self.bound.iter().any(|b| b == name) {
            self.bound.push(name.to_owned());
        }
    }
    fn expr(&mut self, e: &E) {
        match e {
            E::Num(_) => {}
            E::Id(name, pos) => {
                if !self.bound.iter().any(|b| b == name) {
                    self.fail(*pos, format!("Variable '{name}' is not bound"));
                }
            }
            E::Neg(a) | E::Not(a) | E::Complement(a) | E::Factorial(a) => self.expr(a),
            E::Bin(_, a, b) | E::And(a, b) | E::Or(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            E::Ternary(c, a, b) => {
                self.expr(c);
                self.stmt(a);
                self.stmt(b);
            }
            E::Assign(name, _, value, _) => {
                self.bind(name);
                self.expr(value);
            }
            E::Crement { name, pos, .. } => {
                if !self.bound.iter().any(|b| b == name) {
                    self.fail(*pos, format!("Variable '{name}' is not bound"));
                }
            }
            E::Call(name, args, pos) => {
                let Some(arities) = function_arity(name) else {
                    self.fail(*pos, format!("Unknown function '{name}'"));
                    return;
                };
                if !arities.is_empty() && !arities.contains(&args.len()) {
                    let expected = arities.iter().map(usize::to_string).collect::<Vec<_>>().join("/");
                    self.fail(*pos, format!("Incorrect number of arguments for function '{name}', expected {expected}, got {}", args.len()));
                    return;
                }
                for &i in variable_params(name) {
                    if !matches!(args.get(i), Some(E::Id(..))) {
                        self.fail(*pos, format!("Function '{name}' requires a variable in parameter {i}"));
                    }
                }
                for a in args {
                    self.expr(a);
                }
            }
        }
    }
    fn stmt(&mut self, s: &S) {
        match s {
            S::Block(v) => v.iter().for_each(|x| self.stmt(x)),
            S::If(c, a, b) => {
                self.expr(c);
                self.stmt(a);
                if let Some(b) = b {
                    self.stmt(b);
                }
            }
            S::While(c, b, _) => {
                self.expr(c);
                self.stmt(b);
            }
            S::Do(b, c, _) => {
                self.stmt(b);
                self.expr(c);
            }
            S::For { init, cond, update, body, .. } => {
                self.expr(init);
                self.expr(cond);
                self.expr(update);
                self.stmt(body);
            }
            S::Range { counter, first, last, body, .. } => {
                self.bind(counter);
                self.expr(first);
                self.expr(last);
                self.stmt(body);
            }
            S::Return(e) | S::Expr(e) => self.expr(e),
            S::Switch { target, cases, default, .. } => {
                self.expr(target);
                for (_, body) in cases {
                    body.iter().for_each(|x| self.stmt(x));
                }
                if let Some(body) = default {
                    body.iter().for_each(|x| self.stmt(x));
                }
            }
            S::Break | S::Continue | S::Empty => {}
        }
    }
}

// ---- evaluation ----

/// How a statement ended besides having a value.
enum Ctl {
    Break,
    Continue,
    Return(f64),
    Fail(String),
}

type ER = Result<f64, Ctl>;
type SR = Result<Option<f64>, Ctl>;

fn fail<X>(pos: usize, msg: impl std::fmt::Display) -> Result<X, Ctl> {
    Err(Ctl::Fail(format!("Evaluation error at {}: {msg}", pos + 1)))
}

fn constant(name: &str) -> Option<f64> {
    match name {
        "e" => Some(std::f64::consts::E),
        "pi" => Some(std::f64::consts::PI),
        "true" => Some(1.0),
        "false" => Some(0.0),
        _ => None,
    }
}

struct Run<'a> {
    vars: &'a mut HashMap<String, f64>,
    env: &'a dyn ExprEnv,
    deadline: Option<Instant>,
    expr: &'a Expr,
}

impl Run<'_> {
    fn check_deadline(&self) -> Result<(), Ctl> {
        if self.deadline.is_some_and(|d| Instant::now() > d) {
            return Err(Ctl::Fail("Evaluation error: Calculations exceeded time limit.".into()));
        }
        Ok(())
    }

    fn get(&self, name: &str, pos: usize) -> ER {
        if let Some(v) = constant(name) {
            return Ok(v);
        }
        match self.vars.get(name) {
            Some(v) => Ok(*v),
            None => fail(pos, format!("'{name}' is not initialized yet")),
        }
    }

    fn variable(&self, name: &str, pos: usize) -> ER {
        if constant(name).is_some() {
            return fail(pos, format!("'{name}' is not a variable"));
        }
        match self.vars.get(name) {
            Some(v) => Ok(*v),
            None => fail(pos, format!("'{name}' is not initialized yet")),
        }
    }

    fn init(&mut self, name: &str, pos: usize) -> Result<(), Ctl> {
        if constant(name).is_some() {
            return fail(pos, format!("Cannot overwrite non-variable '{name}'"));
        }
        self.vars.entry(name.to_owned()).or_insert(0.0);
        Ok(())
    }

    fn exprs(&mut self, e: &E) -> ER {
        Ok(match e {
            E::Num(v) => *v,
            E::Id(name, pos) => self.get(name, *pos)?,
            E::Neg(a) => -self.exprs(a)?,
            E::Not(a) => f64::from(self.exprs(a)? == 0.0),
            E::Complement(a) => (!(self.exprs(a)? as i64)) as f64,
            E::Factorial(a) => factorial(self.exprs(a)?),
            E::Bin(op, a, b) => {
                let (l, r) = (self.exprs(a)?, self.exprs(b)?);
                binary(*op, l, r)
            }
            E::And(a, b) => {
                if self.exprs(a)? != 0.0 {
                    self.exprs(b)?
                } else {
                    0.0
                }
            }
            E::Or(a, b) => {
                let l = self.exprs(a)?;
                if l != 0.0 {
                    l
                } else {
                    self.exprs(b)?
                }
            }
            E::Ternary(c, a, b) => {
                let branch = if self.exprs(c)? != 0.0 { a } else { b };
                match self.stmt(branch)? {
                    Some(v) => v,
                    None => return fail(0, "Invalid expression for a value"),
                }
            }
            E::Assign(name, op, value, pos) => {
                let arg = self.exprs(value)?;
                let result = match op {
                    None => {
                        self.init(name, *pos)?;
                        arg
                    }
                    Some(op) => {
                        let old = self.variable(name, *pos)?;
                        binary(*op, old, arg)
                    }
                };
                self.vars.insert(name.clone(), result);
                result
            }
            E::Crement { name, delta, post, pos } => {
                let old = self.variable(name, *pos)?;
                self.vars.insert(name.clone(), old + delta);
                if *post {
                    old
                } else {
                    old + delta
                }
            }
            E::Call(name, args, pos) => self.call(name, args, *pos)?,
        })
    }

    /// Runs the body of a loop: its value, whether the loop goes on.
    fn body(&mut self, body: &S, result: &mut Option<f64>) -> Result<bool, Ctl> {
        match self.stmt(body) {
            Ok(v) => {
                *result = v;
                Ok(true)
            }
            Err(Ctl::Break) => Ok(false),
            Err(Ctl::Continue) => Ok(true),
            Err(other) => Err(other),
        }
    }

    fn stmt(&mut self, s: &S) -> SR {
        match s {
            S::Block(v) => self.seq(v),
            S::If(c, a, b) => {
                if self.exprs(c)? != 0.0 {
                    self.stmt(a)
                } else if let Some(b) = b {
                    self.stmt(b)
                } else {
                    Ok(None)
                }
            }
            S::While(c, body, pos) => {
                let (mut result, mut n) = (None, 0u32);
                while self.exprs(c)? != 0.0 {
                    self.iteration(n, *pos)?;
                    n += 1;
                    if !self.body(body, &mut result)? {
                        break;
                    }
                }
                Ok(result)
            }
            S::Do(body, c, pos) => {
                let (mut result, mut n) = (None, 0u32);
                loop {
                    self.iteration(n, *pos)?;
                    n += 1;
                    if !self.body(body, &mut result)? {
                        break;
                    }
                    if self.exprs(c)? == 0.0 {
                        break;
                    }
                }
                Ok(result)
            }
            S::For { init, cond, update, body, pos } => {
                let (mut result, mut n) = (None, 0u32);
                self.exprs(init)?;
                while self.exprs(cond)? != 0.0 {
                    self.iteration(n, *pos)?;
                    n += 1;
                    if !self.body(body, &mut result)? {
                        break;
                    }
                    self.exprs(update)?;
                }
                Ok(result)
            }
            S::Range { counter, first, last, body, pos } => {
                let (first, last) = (self.exprs(first)?, self.exprs(last)?);
                self.init(counter, *pos)?;
                let (mut result, mut n) = (None, 0u32);
                let mut i = first;
                while i <= last {
                    self.iteration(n, *pos)?;
                    n += 1;
                    self.vars.insert(counter.clone(), i);
                    if !self.body(body, &mut result)? {
                        break;
                    }
                    i += 1.0;
                }
                Ok(result)
            }
            S::Break => Err(Ctl::Break),
            S::Continue => Err(Ctl::Continue),
            S::Return(e) => Err(Ctl::Return(self.exprs(e)?)),
            S::Switch { target, cases, default, pos } => {
                let value = self.exprs(target)?;
                let (mut matched, mut falling, mut evaluated) = (false, false, None);
                for (key, body) in cases {
                    if falling || *key == value {
                        matched = true;
                        match self.seq(body) {
                            Ok(v) => {
                                evaluated = v;
                                falling = true;
                            }
                            Err(Ctl::Break) => {
                                falling = false;
                                break;
                            }
                            Err(Ctl::Continue) => return fail(*pos, "Cannot continue in a switch"),
                            Err(other) => return Err(other),
                        }
                    }
                }
                if let Some(body) = default {
                    if falling || !matched {
                        match self.seq(body) {
                            Ok(v) => evaluated = v,
                            Err(Ctl::Break) => {}
                            Err(Ctl::Continue) => return fail(*pos, "Cannot continue in a switch"),
                            Err(other) => return Err(other),
                        }
                    }
                }
                Ok(evaluated)
            }
            S::Expr(e) => Ok(Some(self.exprs(e)?)),
            S::Empty => Ok(None),
        }
    }

    fn seq(&mut self, v: &[S]) -> SR {
        let mut last = None;
        for s in v {
            // An empty statement has no value and does not replace the one before.
            match self.stmt(s)? {
                None if matches!(s, S::Empty) => {}
                value => last = value,
            }
        }
        Ok(last)
    }

    fn iteration(&self, n: u32, pos: usize) -> Result<(), Ctl> {
        if n > MAX_ITERATIONS {
            return fail(pos, "Loop exceeded 256 iterations");
        }
        self.check_deadline()
    }

    fn id_arg(args: &[E], i: usize) -> &str {
        match args.get(i) {
            Some(E::Id(name, _)) => name.as_str(),
            _ => "",
        }
    }

    fn call(&mut self, name: &str, args: &[E], pos: usize) -> ER {
        // Variable arguments are passed by name.
        match name {
            "rotate" => {
                let (xn, yn) = (Self::id_arg(args, 0), Self::id_arg(args, 1));
                let angle = self.exprs(&args[2])?;
                let (x, y) = (self.variable(xn, pos)?, self.variable(yn, pos)?);
                let (s, c) = angle.sin_cos();
                self.vars.insert(xn.to_owned(), x * c - y * s);
                self.vars.insert(yn.to_owned(), x * s + y * c);
                return Ok(0.0);
            }
            "swap" => {
                let (xn, yn) = (Self::id_arg(args, 0), Self::id_arg(args, 1));
                let (x, y) = (self.variable(xn, pos)?, self.variable(yn, pos)?);
                self.vars.insert(xn.to_owned(), y);
                self.vars.insert(yn.to_owned(), x);
                return Ok(0.0);
            }
            "query" | "queryAbs" | "queryRel" => {
                let (x, y, z) = (self.exprs(&args[0])?, self.exprs(&args[1])?, self.exprs(&args[2])?);
                let mode = match name {
                    "query" => 0,
                    "queryAbs" => 1,
                    _ => 2,
                };
                let (type_id, data) = self.env.query(mode, x, y, z);
                // The slots are compared first (-1 matches everything), then the variables among them receive the block's values.
                let mut want = [0.0; 2];
                for i in 0..2 {
                    want[i] = match &args[3 + i] {
                        E::Id(n, p) => self.get(n, *p)?,
                        other => self.exprs(other)?,
                    };
                }
                let hit = (want[0] == -1.0 || type_id == want[0]) && (want[1] == -1.0 || data == want[1]);
                for (i, got) in [type_id, data].into_iter().enumerate() {
                    if let E::Id(n, _) = &args[3 + i] {
                        if constant(n).is_none() {
                            self.vars.insert(n.clone(), got);
                        }
                    }
                }
                return Ok(f64::from(hit));
            }
            _ => {}
        }
        let mut v = Vec::with_capacity(args.len());
        for a in args {
            v.push(self.exprs(a)?);
        }
        let one = |f: fn(f64) -> f64| f(v[0]);
        Ok(match name {
            "sin" => one(f64::sin),
            "cos" => one(f64::cos),
            "tan" => one(f64::tan),
            "asin" => one(f64::asin),
            "acos" => one(f64::acos),
            "atan" => one(f64::atan),
            "sinh" => one(f64::sinh),
            "cosh" => one(f64::cosh),
            "tanh" => one(f64::tanh),
            "sqrt" => one(f64::sqrt),
            "cbrt" => one(f64::cbrt),
            "abs" => one(f64::abs),
            "ceil" => one(f64::ceil),
            "floor" => one(f64::floor),
            "rint" => one(rint),
            "exp" => one(f64::exp),
            "log" | "ln" => one(f64::ln),
            "log10" => one(f64::log10),
            "round" => one(|x| if x.is_nan() { 0.0 } else { (x + 0.5).floor() }),
            "atan2" => v[0].atan2(v[1]),
            "min" | "max" => {
                if v.is_empty() {
                    return fail(pos, "array is empty");
                }
                // `Doubles.min` / `Doubles.max`: a NaN anywhere makes the result NaN.
                let mut best = v[0];
                for &x in &v[1..] {
                    best = if best.is_nan() || x.is_nan() {
                        f64::NAN
                    } else if name == "min" {
                        best.min(x)
                    } else {
                        best.max(x)
                    };
                }
                best
            }
            "random" => noise::random(),
            "randint" => {
                let max = v[0].floor();
                if max < 1.0 {
                    return fail(pos, "bound must be positive");
                }
                (noise::random() * max).floor()
            }
            "megabuf" => {
                let mut buf = self.expr.megabuf.lock().unwrap_or_else(|e| e.into_inner());
                if v.len() == 1 { buffer_get(&mut buf, v[0] as i32) } else { buffer_set(&mut buf, v[0] as i32, v[1]) }
            }
            "gmegabuf" => {
                let mut guard = GLOBAL_MEGABUF.lock().unwrap_or_else(|e| e.into_inner());
                let buf = guard.get_or_insert_with(HashMap::new);
                if v.len() == 1 { buffer_get(buf, v[0] as i32) } else { buffer_set(buf, v[0] as i32, v[1]) }
            }
            "closest" => {
                let mut buf = self.expr.megabuf.lock().unwrap_or_else(|e| e.into_inner());
                find_closest(&mut buf, [v[0], v[1], v[2]], v[3] as i32, v[4] as i32, v[5] as i32)
            }
            "gclosest" => {
                let mut guard = GLOBAL_MEGABUF.lock().unwrap_or_else(|e| e.into_inner());
                find_closest(guard.get_or_insert_with(HashMap::new), [v[0], v[1], v[2]], v[3] as i32, v[4] as i32, v[5] as i32)
            }
            "perlin" => match noise::perlin(v[0] as i32, [v[1], v[2], v[3]], v[4], v[5] as i32, v[6]) {
                Ok(x) => x,
                Err(e) => return fail(0, format!("Perlin noise error: {e}")),
            },
            "voronoi" => match noise::voronoi(v[0] as i32, [v[1], v[2], v[3]], v[4]) {
                Ok(x) => x,
                Err(e) => return fail(0, format!("Voronoi error: {e}")),
            },
            "ridgedmulti" => match noise::ridged_multi(v[0] as i32, [v[1], v[2], v[3]], v[4], v[5] as i32) {
                Ok(x) => x,
                Err(e) => return fail(0, format!("Ridged multi error: {e}")),
            },
            _ => return fail(pos, format!("Unknown function '{name}'")),
        })
    }
}

fn buffer_get(buf: &mut Buffer, index: i32) -> f64 {
    buf.entry(index & !1023).or_insert_with(|| Box::new([0.0; 1024]))[(index & 1023) as usize]
}

fn buffer_set(buf: &mut Buffer, index: i32, value: f64) -> f64 {
    buf.entry(index & !1023).or_insert_with(|| Box::new([0.0; 1024]))[(index & 1023) as usize] = value;
    value
}

/// `Functions.findClosest`: the index of the point nearest to (x, y, z) among `count` triples starting at `index`.
fn find_closest(buf: &mut Buffer, at: [f64; 3], mut index: i32, count: i32, stride: i32) -> f64 {
    let (mut closest, mut best) = (-1, f64::MAX);
    for _ in 0..count.max(0) {
        let d = [buffer_get(buf, index) - at[0], buffer_get(buf, index.wrapping_add(1)) - at[1], buffer_get(buf, index.wrapping_add(2)) - at[2]];
        let dist = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if dist < best {
            best = dist;
            closest = index;
        }
        index = index.wrapping_add(stride);
    }
    f64::from(closest)
}

/// `Math.rint`: the nearest integer, ties to even.
fn rint(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

fn factorial(x: f64) -> f64 {
    let n = x as i32;
    if n < 0 {
        return 0.0;
    }
    if n >= 171 {
        return f64::INFINITY;
    }
    (1..=n).fold(1.0, |a, i| a * f64::from(i))
}

/// Usable AlmostEqual, as in `CompilingVisitor.almostEqual2sComplement`.
fn almost_equal(a: f64, b: f64) -> bool {
    let fix = |v: f64| {
        let l = v.to_bits() as i64;
        if l < 0 { i64::MIN.wrapping_sub(l) } else { l }
    };
    fix(a).wrapping_sub(fix(b)).unsigned_abs() <= 450_359_963
}

fn binary(op: Op, l: f64, r: f64) -> f64 {
    match op {
        Op::Add => l + r,
        Op::Sub => l - r,
        Op::Mul => l * r,
        Op::Div => l / r,
        Op::Rem => l % r,
        Op::Pow => l.powf(r),
        Op::Shl => ((l as i64) << ((r as i64) & 63)) as f64,
        Op::Shr => ((l as i64) >> ((r as i64) & 63)) as f64,
        Op::Lt => f64::from(l < r),
        Op::Le => f64::from(l <= r),
        Op::Gt => f64::from(l > r),
        Op::Ge => f64::from(l >= r),
        Op::Eq => f64::from(l == r),
        Op::Ne => f64::from(l != r),
        Op::Near => f64::from(almost_equal(l, r)),
    }
}

impl Expr {
    /// Parses `text`; the names in `known` are the variables the caller provides.
    pub fn parse_with(text: &str, known: &[&str]) -> Result<Expr, String> {
        if text.is_empty() {
            return Err("Expression cannot be empty string.".into());
        }
        let mut p = P { t: lex(text)?, at: 0 };
        let program = p.statements_until(None)?;
        if !matches!(p.peek(), T::End) {
            return Err(perr(p.pos(), &format!("extraneous input {}", p.describe())));
        }
        let mut v = Validator::new(known);
        program.iter().for_each(|s| v.stmt(s));
        if let Some(problem) = v.problem {
            return Err(problem);
        }
        Ok(Expr { program, megabuf: Mutex::new(HashMap::new()) })
    }

    /// Parses `text` with no variables given by the caller.
    pub fn parse(text: &str) -> Result<Expr, String> {
        Self::parse_with(text, &[])
    }

    /// Runs the expression: `vars` holds the variables the caller provides and receives every assignment. The value of the last
    /// statement.
    pub fn run(&self, vars: &mut HashMap<String, f64>) -> Result<f64, String> {
        self.run_in(vars, &NoEnvPublic, None)
    }

    /// [`Expr::run`] with an environment for `query` and a time limit in milliseconds (negative: none).
    pub fn run_in(&self, vars: &mut HashMap<String, f64>, env: &dyn ExprEnv, timeout_ms: Option<i64>) -> Result<f64, String> {
        let deadline = timeout_ms.filter(|t| *t >= 0).map(|t| Instant::now() + std::time::Duration::from_millis(t as u64));
        let mut run = Run { vars, env, deadline, expr: self };
        match run.seq(&self.program) {
            Ok(Some(v)) | Err(Ctl::Return(v)) => Ok(v),
            Ok(None) => Err("Evaluation error: Expression must result in a value".into()),
            Err(Ctl::Break | Ctl::Continue) => Err("Evaluation error: break or continue outside a loop".into()),
            Err(Ctl::Fail(e)) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, vars: &[(&str, f64)]) -> f64 {
        let mut m: HashMap<String, f64> = vars.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let names: Vec<&str> = vars.iter().map(|(k, _)| *k).collect();
        Expr::parse_with(text, &names).unwrap().run(&mut m).unwrap()
    }

    fn err(text: &str) -> String {
        match Expr::parse(text) {
            Err(e) => e,
            Ok(e) => e.run(&mut HashMap::new()).unwrap_err(),
        }
    }

    #[test]
    fn arithmetic_comparisons_and_variables() {
        assert_eq!(run("2+3*4", &[]), 14.0);
        assert_eq!(run("x*x + y*y < 1", &[("x", 0.5), ("y", 0.5)]), 1.0);
        assert_eq!(run("x*x + y*y < 1", &[("x", 1.0), ("y", 1.0)]), 0.0);
        assert_eq!(run("a = 3; a += 2; a * 2", &[]), 10.0);
        assert_eq!(run("x > 0 ? 7 : 9", &[("x", -1.0)]), 9.0);
        assert_eq!(run("!0 && 1 || 0", &[]), 1.0);
        assert_eq!(run("sqrt(x^2 + 0) == 3", &[("x", 3.0)]), 1.0);
    }

    #[test]
    fn power_is_left_associative_and_unary_binds_tighter() {
        // `powerExpression POWER unaryExpression`: (2^3)^2, and (-2)^2.
        assert_eq!(run("2^3^2", &[]), 64.0);
        assert_eq!(run("-2^2", &[]), 4.0);
        assert_eq!(run("2**3", &[]), 8.0);
    }

    #[test]
    fn remainder_follows_the_dividend_like_java() {
        assert_eq!(run("-7 % 3", &[]), -1.0);
        assert_eq!(run("7 % 3", &[]), 1.0);
    }

    #[test]
    fn statements_and_loops() {
        assert_eq!(run("s = 0; for (i = 1, 4) { s += i } s", &[]), 10.0);
        assert_eq!(run("s = 0; i = 0; while (i < 5) { i++; if (i == 3) continue; s += i } s", &[]), 12.0);
        assert_eq!(run("s = 0; do { s++ } while (s < 3); s", &[]), 3.0);
        assert_eq!(run("s = 0; for (i = 0; i < 10; i++) { if (i > 3) break; s += i } s", &[]), 6.0);
        assert_eq!(run("return 5; 6", &[]), 5.0);
        assert_eq!(run("if (x) 1 else 2", &[("x", 0.0)]), 2.0);
        // The break leaves before the second body gives its value: the first one stays.
        assert_eq!(run("switch (x) { case 1: 10; case 2: 20; break; default: 30 }", &[("x", 1.0)]), 10.0);
        assert_eq!(run("switch (x) { case 1: 10; break; default: 30 }", &[("x", 7.0)]), 30.0);
    }

    #[test]
    fn operators_of_the_grammar() {
        assert_eq!(run("1 << 4", &[]), 16.0);
        assert_eq!(run("-16 >> 2", &[]), -4.0);
        assert_eq!(run("~5", &[]), -6.0);
        assert_eq!(run("5!", &[]), 120.0);
        assert_eq!(run("0.1 + 0.2 ~= 0.3", &[]), 1.0);
        assert_eq!(run("0.1 + 0.2 == 0.3", &[]), 0.0);
        assert_eq!(run("a = 1; b = a++ + ++a; b * 10 + a", &[]), 43.0);
    }

    #[test]
    fn functions_and_buffers() {
        assert_eq!(run("max(1, 7, 3) + min(4, 2)", &[]), 9.0);
        assert_eq!(run("round(2.5) + rint(2.5)", &[]), 5.0);
        assert_eq!(run("megabuf(3, 9); megabuf(3) + megabuf(4)", &[]), 9.0);
        assert_eq!(run("gmegabuf(1030, 4); gmegabuf(1030)", &[]), 4.0);
        assert_eq!(run("megabuf(0,1);megabuf(1,1);megabuf(2,1);megabuf(3,5);megabuf(4,5);megabuf(5,5); closest(4,4,4, 0, 2, 3)", &[]), 3.0);
        let mut m: HashMap<String, f64> = HashMap::from([("x".into(), 1.0), ("y".into(), 2.0)]);
        Expr::parse_with("swap(x, y)", &["x", "y"]).unwrap().run(&mut m).unwrap();
        assert_eq!((m["x"], m["y"]), (2.0, 1.0));
        let mut m: HashMap<String, f64> = HashMap::from([("x".into(), 1.0), ("y".into(), 0.0)]);
        Expr::parse_with("rotate(x, y, pi/2)", &["x", "y"]).unwrap().run(&mut m).unwrap();
        assert!(m["x"].abs() < 1e-12 && (m["y"] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn identifiers_are_case_sensitive_and_constants_stay() {
        assert_eq!(run("pi > 3", &[]), 1.0);
        assert!(err("Pi").contains("Variable 'Pi' is not bound"));
        assert!(err("pi = 3").contains("Cannot overwrite non-variable 'pi'"));
        assert_eq!(run("x = 4; X = 5; x", &[]), 4.0);
    }

    #[test]
    fn errors_name_position_and_problem() {
        assert!(err("2 +").starts_with("Parser error at 4"));
        // Statements need no separator: `1 2` is two statements and gives 2.
        assert_eq!(run("1 2", &[]), 2.0);
        assert_eq!(err("foo + 1"), "Evaluation error at 1: Variable 'foo' is not bound");
        assert_eq!(err("bar(1)"), "Evaluation error at 1: Unknown function 'bar'");
        assert_eq!(err("sin(1, 2)"), "Evaluation error at 1: Incorrect number of arguments for function 'sin', expected 1, got 2");
        assert_eq!(err("rotate(1, 2, 3)"), "Evaluation error at 1: Function 'rotate' requires a variable in parameter 0");
        assert_eq!(err("i = 0; while (1) { i++ }"), "Evaluation error at 8: Loop exceeded 256 iterations");
        assert!(err("if (0) 1").contains("Expression must result in a value"));
        assert!(err("a = 1; a +").starts_with("Parser error"));
    }

    #[test]
    fn a_time_limit_stops_a_long_run() {
        let e = Expr::parse("i = 0; while (1) { for (j = 1, 200) { i++ } }").unwrap();
        let mut m = HashMap::new();
        assert!(e.run_in(&mut m, &NoEnvPublic, Some(0)).is_err());
    }

    #[test]
    fn query_reads_the_environment_and_fills_its_slots() {
        struct Env;
        impl ExprEnv for Env {
            fn query(&self, mode: u8, _x: f64, _y: f64, _z: f64) -> (f64, f64) {
                if mode == 1 { (1.0, 0.0) } else { (-1.0, -1.0) }
            }
        }
        let e = Expr::parse_with("queryAbs(1, 2, 3, t, d) + t * 10", &[]).err().unwrap();
        assert!(e.contains("Variable 't' is not bound"));
        let e = Expr::parse_with("t = -1; d = -1; queryAbs(1, 2, 3, t, d) + t * 10", &[]).unwrap();
        let mut m = HashMap::new();
        assert_eq!(e.run_in(&mut m, &Env, None).unwrap(), 11.0);
        assert_eq!((m["t"], m["d"]), (1.0, 0.0));
    }
}

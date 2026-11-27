//! Stringified NBT (`{Items:[{id:"minecraft:stone",Count:1b}]}`), as block patterns carry it (`chest{...}`). The reading side of
//! `LinStringIO`: compounds, lists, typed arrays, quoted and bare strings, and numbers with the type suffixes `b s l f d`.

use crate::horizon_worldedit_core::nbt::Tag;
use std::collections::BTreeMap;

struct P<'a> {
    c: Vec<char>,
    at: usize,
    text: &'a str,
}

type R<T> = Result<T, String>;

/// Reads one compound; `Err` says what is wrong and where.
pub fn parse_compound(text: &str) -> R<Tag> {
    let mut p = P { c: text.chars().collect(), at: 0, text };
    p.skip();
    let tag = p.value()?;
    p.skip();
    if p.at < p.c.len() {
        return Err(format!("Trailing data at position {}", p.at));
    }
    match tag {
        Tag::Compound(_) => Ok(tag),
        _ => Err("Expected a compound".into()),
    }
}

impl P<'_> {
    fn skip(&mut self) {
        while self.c.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn err<T>(&self, what: &str) -> R<T> {
        Err(format!("{what} at position {} of '{}'", self.at, self.text))
    }

    fn eat(&mut self, ch: char) -> bool {
        self.skip();
        if self.c.get(self.at) == Some(&ch) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> R<Tag> {
        self.skip();
        match self.c.get(self.at) {
            Some('{') => self.compound(),
            Some('[') => self.list(),
            Some('"' | '\'') => Ok(Tag::String(self.quoted()?)),
            Some(_) => self.bare(),
            None => self.err("Unexpected end"),
        }
    }

    fn compound(&mut self) -> R<Tag> {
        self.at += 1;
        let mut map = BTreeMap::new();
        if self.eat('}') {
            return Ok(Tag::Compound(map));
        }
        loop {
            self.skip();
            let key = match self.c.get(self.at) {
                Some('"' | '\'') => self.quoted()?,
                _ => {
                    let start = self.at;
                    while self.c.get(self.at).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')) {
                        self.at += 1;
                    }
                    if start == self.at {
                        return self.err("Expected a key");
                    }
                    self.c[start..self.at].iter().collect()
                }
            };
            if !self.eat(':') {
                return self.err("Expected ':'");
            }
            let v = self.value()?;
            map.insert(key, v);
            if self.eat(',') {
                continue;
            }
            if self.eat('}') {
                return Ok(Tag::Compound(map));
            }
            return self.err("Expected ',' or '}'");
        }
    }

    fn list(&mut self) -> R<Tag> {
        self.at += 1;
        self.skip();
        // `[B; 1b, 2b]`, `[I; 1, 2]`, `[L; 1l]`.
        if let (Some(kind), Some(';')) = (self.c.get(self.at).copied(), self.c.get(self.at + 1)) {
            if matches!(kind, 'B' | 'I' | 'L') {
                self.at += 2;
                let mut items: Vec<Tag> = Vec::new();
                if !self.eat(']') {
                    loop {
                        items.push(self.value()?);
                        if self.eat(',') {
                            continue;
                        }
                        if self.eat(']') {
                            break;
                        }
                        return self.err("Expected ',' or ']'");
                    }
                }
                let num = |t: &Tag| -> Option<i64> {
                    match t {
                        Tag::Byte(v) => Some(i64::from(*v)),
                        Tag::Short(v) => Some(i64::from(*v)),
                        Tag::Int(v) => Some(i64::from(*v)),
                        Tag::Long(v) => Some(*v),
                        _ => None,
                    }
                };
                let nums: Option<Vec<i64>> = items.iter().map(num).collect();
                let Some(nums) = nums else { return self.err("Expected numbers in the array") };
                return Ok(match kind {
                    'B' => Tag::ByteArray(nums.into_iter().map(|n| n as i8).collect()),
                    'I' => Tag::IntArray(nums.into_iter().map(|n| n as i32).collect()),
                    _ => Tag::LongArray(nums),
                });
            }
        }
        let mut items = Vec::new();
        if self.eat(']') {
            return Ok(Tag::List(items));
        }
        loop {
            items.push(self.value()?);
            if self.eat(',') {
                continue;
            }
            if self.eat(']') {
                return Ok(Tag::List(items));
            }
            return self.err("Expected ',' or ']'");
        }
    }

    fn quoted(&mut self) -> R<String> {
        let quote = self.c[self.at];
        self.at += 1;
        let mut out = String::new();
        while let Some(&ch) = self.c.get(self.at) {
            self.at += 1;
            if ch == '\\' {
                match self.c.get(self.at) {
                    Some(&n) => {
                        out.push(n);
                        self.at += 1;
                    }
                    None => return self.err("Unterminated string"),
                }
            } else if ch == quote {
                return Ok(out);
            } else {
                out.push(ch);
            }
        }
        self.err("Unterminated string")
    }

    fn bare(&mut self) -> R<Tag> {
        let start = self.at;
        while self.c.get(self.at).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | ':')) {
            self.at += 1;
        }
        if start == self.at {
            return self.err("Unexpected character");
        }
        let word: String = self.c[start..self.at].iter().collect();
        Ok(number(&word).unwrap_or(Tag::String(word)))
    }
}

/// A number with its type suffix (`1b`, `2s`, `3` an int, `4l`, `5f`, `6.5` and `6.5d` doubles), or `true`/`false` as bytes.
fn number(word: &str) -> Option<Tag> {
    match word {
        "true" => return Some(Tag::Byte(1)),
        "false" => return Some(Tag::Byte(0)),
        _ => {}
    }
    let last = word.chars().last()?.to_ascii_lowercase();
    let body = &word[..word.len() - 1];
    let digits = |s: &str| !s.is_empty() && s.trim_start_matches(['+', '-']).chars().all(|c| c.is_ascii_digit());
    match last {
        'b' if digits(body) => body.parse().ok().map(Tag::Byte),
        's' if digits(body) => body.parse().ok().map(Tag::Short),
        'l' if digits(body) => body.parse().ok().map(Tag::Long),
        'f' if body.parse::<f32>().is_ok() && body.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.') => body.parse().ok().map(Tag::Float),
        'd' if body.parse::<f64>().is_ok() && body.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.') => body.parse().ok().map(Tag::Double),
        _ if digits(word) => word.parse().ok().map(Tag::Int),
        _ if word.parse::<f64>().is_ok() && word.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.') => word.parse().ok().map(Tag::Double),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compounds_lists_and_numbers() {
        let t = parse_compound("{Items:[{id:\"minecraft:stone\",Count:2b,Slot:0}],CustomName:'x',Pos:[1.5d,2.0,3f],ok:true,Arr:[I;1,2],long:5L}").unwrap();
        let items = t.get("Items").and_then(Tag::list).unwrap();
        assert_eq!(items[0].get("Count"), Some(&Tag::Byte(2)));
        assert_eq!(items[0].get("Slot"), Some(&Tag::Int(0)));
        assert_eq!(items[0].get("id").and_then(Tag::str), Some("minecraft:stone"));
        assert_eq!(t.get("Pos").and_then(Tag::list).unwrap(), &[Tag::Double(1.5), Tag::Double(2.0), Tag::Float(3.0)]);
        assert_eq!(t.get("ok"), Some(&Tag::Byte(1)));
        assert_eq!(t.get("Arr"), Some(&Tag::IntArray(vec![1, 2])));
        assert_eq!(t.get("long"), Some(&Tag::Long(5)));
        assert_eq!(parse_compound("{}").unwrap(), Tag::Compound(BTreeMap::new()));
    }

    #[test]
    fn broken_input_says_where() {
        assert!(parse_compound("{a:1").is_err());
        assert!(parse_compound("{a 1}").unwrap_err().contains("Expected ':'"));
        assert!(parse_compound("[1]").is_err());
        assert!(parse_compound("{a:1} x").unwrap_err().contains("Trailing"));
    }
}

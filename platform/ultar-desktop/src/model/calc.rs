//! The calculator of the overview search (`search.source.calculator`): a small, safe
//! evaluator of `+ - * / × ÷ ( )` on decimal numbers. Arabic-Indic digits and the Arabic
//! decimal comma are accepted. Split out of `search.rs` so each stays a small module.

use ultar_i18n::numbers;

/// Evaluates `+ - * / × ÷ ( )` on decimal numbers (Arabic-Indic digits and the Arabic
/// decimal comma are accepted); `None` for anything else or a single plain number.
pub fn calculate(text: &str) -> Option<String> {
    let text = numbers::to_western_digits(text)
        .replace('٫', ".")
        .replace('×', "*")
        .replace('÷', "/");
    if !text.chars().any(|c| matches!(c, '+' | '-' | '*' | '/')) {
        return None;
    }
    let tokens: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser {
        t: &tokens,
        i: 0,
        depth: 0,
    };
    let value = p.sum()?;
    if p.i != tokens.len() || !value.is_finite() || value.abs() > 1e15 {
        return None;
    }
    let rounded = (value * 1e9).round() / 1e9;
    let mut s = format!("{rounded}");
    if s == "-0" {
        s = "0".into();
    }
    Some(s)
}

struct Parser<'a> {
    t: &'a [char],
    i: usize,
    depth: u8,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.t.get(self.i).copied()
    }

    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let r = self.product()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.atom()?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.i += 1;
            let r = self.atom()?;
            if op == '/' && r == 0.0 {
                return None;
            }
            v = if op == '*' { v * r } else { v / r };
        }
        Some(v)
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            '(' if self.depth < 16 => {
                self.i += 1;
                self.depth += 1;
                let v = self.sum()?;
                self.depth -= 1;
                (self.peek() == Some(')')).then(|| self.i += 1)?;
                Some(v)
            }
            '-' => {
                self.i += 1;
                Some(-self.atom()?)
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.')
                    && self.i - start < 24
                {
                    self.i += 1;
                }
                self.t[start..self.i]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .ok()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calculator_is_small_and_safe() {
        assert_eq!(calculate("1+2*3").as_deref(), Some("7"));
        assert_eq!(calculate("(1+2)*3").as_deref(), Some("9"));
        assert_eq!(calculate("١٠ ÷ ٤").as_deref(), Some("2.5"));
        assert_eq!(calculate("-3+1").as_deref(), Some("-2"));
        assert_eq!(calculate("0.1+0.2").as_deref(), Some("0.3"));
        for bad in [
            "42",
            "1/0",
            "1+",
            "(1+2",
            "a+b",
            "1e400*1e400",
            &"(".repeat(40),
            "9999999999999999*9999",
        ] {
            assert_eq!(calculate(bad), None, "{bad}");
        }
    }
}

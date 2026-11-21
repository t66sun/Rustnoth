//! Evaluate the gettext Plural-Forms expressions shipped in Wesnoth's MO catalogs.
pub(super) fn index(rule: &str, count: u64) -> Option<usize> {
    let mut parser = Parser { rest: rule, count };
    let form = parser.conditional()?;
    if !parser.rest.trim().is_empty() { return None; }
    usize::try_from(form).ok()
}

struct Parser<'a> {
    rest: &'a str,
    count: u64,
}

impl Parser<'_> {
    fn take(&mut self, token: &str) -> bool {
        if let Some(rest) = self.rest.trim_start().strip_prefix(token) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn atom(&mut self) -> Option<u64> {
        if self.take("(") {
            let value = self.conditional()?;
            return self.take(")").then_some(value);
        }
        if self.take("n") { return Some(self.count); }
        if self.take("!") { return Some(u64::from(self.atom()? == 0)); }
        let rest = self.rest.trim_start();
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let value = rest[..digits].parse().ok()?;
        self.rest = &rest[digits..];
        Some(value)
    }

    fn binary(&mut self, min_precedence: u8) -> Option<u64> {
        // Longer operators precede their one-character prefixes.
        const OPS: &[(&str, u8)] = &[
            ("||", 1), ("&&", 2), ("==", 3), ("!=", 3), ("<=", 4), (">=", 4),
            ("<", 4), (">", 4), ("%", 5),
        ];
        let mut left = self.atom()?;
        while let Some(&(op, precedence)) = OPS.iter().find(|(op, _)| self.rest.trim_start().starts_with(*op)) {
            if precedence < min_precedence { break; }
            self.take(op);
            let right = self.binary(precedence + 1)?;
            left = match op {
                "||" => u64::from(left != 0 || right != 0),
                "&&" => u64::from(left != 0 && right != 0),
                "==" => u64::from(left == right),
                "!=" => u64::from(left != right),
                "<=" => u64::from(left <= right),
                ">=" => u64::from(left >= right),
                "<" => u64::from(left < right),
                ">" => u64::from(left > right),
                "%" => left.checked_rem(right)?,
                _ => unreachable!(),
            };
        }
        Some(left)
    }

    fn conditional(&mut self) -> Option<u64> {
        let condition = self.binary(1)?;
        if !self.take("?") { return Some(condition); }
        let yes = self.conditional()?;
        if !self.take(":") { return None; }
        let no = self.conditional()?;
        Some(if condition != 0 { yes } else { no })
    }
}

use std::collections::BTreeMap;

use crate::wml::Node;

/// Base percentages of weapon damage taken by a movement type.
#[derive(Debug, Default)]
pub struct DamageTable {
    percentages: BTreeMap<String, i32>,
}

impl DamageTable {
    /// Reads the attributes of a preprocessed WML `[resistance]` tag.
    pub fn from_wml(table: &Node) -> Self {
        let percentages = table
            .attributes
            .iter()
            .filter(|(_, value)| value.translations.is_empty())
            .filter_map(|(damage_type, value)| {
                Some((damage_type.clone(), read_percentage(&value.text)?))
            })
            .collect();
        Self { percentages }
    }

    /// Returns the damage taken in percent. Unset damage types deal 100%.
    pub fn damage_taken(&self, damage_type: &str) -> i32 {
        self.percentages.get(damage_type).copied().unwrap_or(100)
    }
}

fn read_percentage(text: &str) -> Option<i32> {
    // WML stores canonical positive integers as unsigned values and narrows
    // them on lookup. String-backed values take the leading decimal integer.
    if matches!(text.as_bytes().first(), Some(b'1'..=b'9'))
        && let Ok(value) = text.parse::<u64>()
    {
        return Some(value as i32);
    }
    let text = text.trim_ascii_start();
    let sign = usize::from(text.starts_with(['+', '-']));
    let digits = text.as_bytes()[sign..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    text[..sign + digits].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::DamageTable;
    use crate::wml;

    #[test]
    fn reads_damage_taken_percentages_without_clamping() {
        let wml = wml::parse("blade=70\nfire=150\narcane=0\ncold=-20\nsonic=92").unwrap();
        let table = DamageTable::from_wml(&wml);
        assert_eq!(table.damage_taken("blade"), 70);
        assert_eq!(table.damage_taken("fire"), 150);
        assert_eq!(table.damage_taken("arcane"), 0);
        assert_eq!(table.damage_taken("cold"), -20);
        assert_eq!(table.damage_taken("sonic"), 92);
        assert_eq!(table.damage_taken("Sonic"), 100);
        assert_eq!(DamageTable::default().damage_taken("fire"), 100);
    }

    #[test]
    fn follows_wml_integer_conversion_and_uses_normal_damage_as_fallback() {
        let wml = wml::parse(
            "fire=\" +42 extra\"\nblade=4294967376\nempty=\ncold=invalid\narcane=yes\npierce=_ \"33\"",
        )
        .unwrap();
        let table = DamageTable::from_wml(&wml);
        assert_eq!(table.damage_taken("fire"), 42);
        assert_eq!(table.damage_taken("blade"), 80);
        for damage_type in ["empty", "cold", "arcane", "pierce"] {
            assert_eq!(table.damage_taken(damage_type), 100);
        }
    }
}

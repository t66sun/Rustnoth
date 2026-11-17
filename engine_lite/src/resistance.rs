use std::collections::BTreeMap;

use crate::wml::Node;

/// Percentages of weapon damage taken by a movement or unit type.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DamageTable {
    percentages: BTreeMap<String, i32>,
}

impl DamageTable {
    /// Reads the attributes of a preprocessed WML `[resistance]` tag.
    pub fn from_wml(table: &Node) -> Self {
        Self::from_wml_tables([table])
    }

    /// Reads tables in declaration order, retaining the last value for each damage type.
    pub(crate) fn from_wml_tables<'a>(tables: impl IntoIterator<Item = &'a Node>) -> Self {
        let mut percentages = BTreeMap::new();
        for table in tables {
            for (damage_type, value) in &table.attributes {
                let percentage = if value.translations.is_empty() {
                    read_percentage(&value.text).unwrap_or(100)
                } else {
                    100
                };
                percentages.insert(damage_type.clone(), percentage);
            }
        }
        Self { percentages }
    }

    /// Returns the damage taken in percent. Unset damage types deal 100%.
    pub fn damage_taken(&self, damage_type: &str) -> i32 {
        self.damage_taken_with_fallback(damage_type, None)
    }

    pub(crate) fn damage_taken_with_fallback(
        &self,
        damage_type: &str,
        fallback: Option<&Self>,
    ) -> i32 {
        self.percentages
            .get(damage_type)
            .or_else(|| fallback?.percentages.get(damage_type))
            .copied()
            .unwrap_or(100)
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
    use crate::{
        board::AttackChoice, fixed_match::Action, hex::Hex, scenario_bundle::Bundle, wml,
    };

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

    #[test]
    fn combat_uses_unit_resistances_over_inherited_values() {
        let bundle = Bundle::parse(
            br#"
            [units]
                [movetype]
                    name=foot
                    [resistance]
                        blade=90
                        fire=70
                        cold=60
                    [/resistance]
                [/movetype]
                [unit_type]
                    id=Striker
                    hitpoints=20
                    [attack]
                        type=blade
                        range=melee
                        damage=10
                        number=1
                    [/attack]
                    [attack]
                        type=fire
                        range=melee
                        damage=10
                        number=1
                    [/attack]
                    [attack]
                        type=cold
                        range=melee
                        damage=10
                        number=1
                    [/attack]
                [/unit_type]
                [unit_type]
                    id=Guard
                    hitpoints=20
                    movement_type=foot
                    [resistance]
                        blade=40
                        cold=invalid
                    [/resistance]
                    [resistance]
                        blade=50
                    [/resistance]
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Striker
                [/side]
                [side]
                    side=2
                    type=Guard
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        for (weapon, expected_hitpoints) in [(0, 15), (1, 13), (2, 10)] {
            let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
            game.step(Action::Attack(AttackChoice { from, to, weapon })).unwrap();
            let snapshot = game.audit_snapshot();
            let target = snapshot.units.iter().find(|unit| unit.position == to).unwrap();
            assert_eq!(target.hitpoints, expected_hitpoints);
        }
    }
}

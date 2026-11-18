use crate::{scenario_sides::SideConfig, wml::Node};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Upkeep {
    #[default]
    Full,
    Fixed(i32),
}

impl Upkeep {
    pub(crate) fn from_wml(unit: &Node) -> Option<Self> {
        let value = unit.attributes.get("upkeep").filter(|v| !v.text.is_empty())?;
        Some(match value.text.as_str() {
            _ if !value.translations.is_empty() => Self::Full,
            "loyal" | "free" => Self::Fixed(0),
            "full" => Self::Full,
            raw => raw.parse::<f64>().ok()
                .filter(|n| n.is_finite() && *n >= 0.0 && n.to_string() == raw)
                .map_or(Self::Full, |n| Self::Fixed(n as i32)),
        })
    }

    pub(crate) fn amount(self, level: i32) -> i32 {
        match self {
            Self::Full => level,
            Self::Fixed(value) => value,
        }
    }
}

/// Wesnoth's default global base income.
pub const DEFAULT_BASE_INCOME: i32 = 2;

/// Returns the gold change for an income-paying side turn.
///
/// The caller supplies the global base income and the side's total unit
/// upkeep. Wesnoth applies this change when the current round is greater than
/// one.
pub fn net_income(
    side: &SideConfig,
    base_income: i32,
    village_count: usize,
    total_upkeep: i32,
) -> i64 {
    let villages = village_count as i64;
    let village_income = villages * i64::from(side.village_gold);
    let village_support = villages * i64::from(side.village_support);
    let expenses = (i64::from(total_upkeep) - village_support).max(0);
    i64::from(base_income) + i64::from(side.income_bonus) + village_income - expenses
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scenario_sides::read_sides, wml};

    #[test]
    fn round_two_income_uses_unit_upkeep_after_traits_and_authored_overrides() {
        use crate::{fixed_match::Action, scenario_bundle::Bundle};

        let loyal = "[modifications]\n[trait]\nid=loyal\n[effect]\napply_to=loyal\n[/effect]\n[/trait]\n[/modifications]";
        for (attributes, expected, traits) in [
            ("", 98, false),
            ("upkeep=loyal", 102, false),
            ("upkeep=free", 102, false),
            ("upkeep=0", 102, false),
            ("upkeep=3", 99, false),
            ("upkeep=full", 100, false),
            (loyal, 102, true),
            (&format!("{loyal}\nupkeep=3"), 99, true),
        ] {
            let source = format!(r#"
[units]
    [unit_type]
        id=Swordsman
        hitpoints=55
        level=2
        upkeep=4
    [/unit_type]
[/units]
[scenario]
    map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
    [side]
        side=1
        type=Swordsman
        gold=100
        [unit]
            type=Swordsman
            x,y=2,1
            {attributes}
        [/unit]
    [/side]
    [side]
        side=2
        type=Swordsman
    [/side]
[/scenario]"#);
            let bundle = Bundle::parse(source.as_bytes()).unwrap();
            let mut game = bundle.start(1, |_| unreachable!()).unwrap();
            game.step(Action::EndTurn).unwrap();
            game.step(Action::EndTurn).unwrap();
            let snapshot = game.audit_snapshot();
            assert_eq!(snapshot.gold[0], expected, "{attributes}");
            assert_eq!(snapshot.units[1].traits == ["loyal"], traits, "{attributes}");
        }
    }

    #[test]
    fn villages_add_income_and_cover_upkeep_without_refunding_surplus_support() {
        let scenario = wml::parse(
            "[side]\nside=1\nincome=-1\nvillage_gold=3\nvillage_support=2\n[/side]",
        )
        .unwrap();
        let sides = read_sides(&scenario);
        assert_eq!(net_income(&sides[0], DEFAULT_BASE_INCOME, 3, 9), 7);
        assert_eq!(net_income(&sides[0], DEFAULT_BASE_INCOME, 3, 4), 10);
    }

    #[test]
    fn excess_upkeep_can_leave_a_negative_gold_change() {
        let scenario = wml::parse("[side]\nside=1\nincome=-3\n[/side]").unwrap();
        let sides = read_sides(&scenario);
        assert_eq!(net_income(&sides[0], 4, 0, 6), -5);
    }
}

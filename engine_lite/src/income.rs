use crate::scenario_sides::SideConfig;

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

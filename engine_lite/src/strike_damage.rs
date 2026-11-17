/// Compute strike damage after modifiers and slow, using Wesnoth's rounding.
///
/// `multiplier` is in ten-thousandths. For example, a 25% alignment bonus
/// against a target that receives 80% damage gives `125 * 80 = 10000`.
pub fn strike_damage(base: i32, multiplier: i32, slowed: bool) -> i64 {
    if base == 0 {
        return 0;
    }

    let divisor = if slowed { 20_000 } else { 10_000 };
    let multiplier = i64::from(multiplier);
    let product = i64::from(base) * multiplier;
    let whole = product / divisor;
    let fraction = product % divisor;
    // On a tie, go toward the unmodified base damage.
    let above_half = fraction * 2 > divisor;
    let tie_below_base = fraction * 2 == divisor && multiplier < divisor;
    (whole + i64::from(above_half || tie_below_base)).max(1)
}

#[cfg(test)]
mod tests {
    use super::strike_damage;

    #[test]
    fn ties_round_toward_the_base_damage() {
        assert_eq!(strike_damage(6, 7_500, false), 5);
        assert_eq!(strike_damage(6, 12_500, false), 7);
        assert_eq!(strike_damage(6, 12_501, false), 8);
    }

    #[test]
    fn slow_is_applied_before_rounding() {
        assert_eq!(strike_damage(3, 10_000, true), 2);
        assert_eq!(strike_damage(6, 11_000, false), 7);
        assert_eq!(strike_damage(6, 11_000, true), 3);
    }

    #[test]
    fn only_zero_base_damage_can_round_to_zero() {
        assert_eq!(strike_damage(0, 20_000, false), 0);
        assert_eq!(strike_damage(1, 0, true), 1);
        assert_eq!(strike_damage(4, -1_000, false), 1);
    }
}

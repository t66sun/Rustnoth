use crate::wml::Node;

/// The combat fields of a preprocessed Wesnoth 1.18.8 `[attack]` tag.
#[derive(Debug, PartialEq, Eq)]
pub struct WeaponProfile {
    pub id: String,
    pub damage_type: String,
    pub range: String,
    pub min_range: i32,
    pub max_range: i32,
    pub damage: i32,
    pub strikes: i32,
    pub accuracy: i32,
    pub parry: i32,
    pub movement_used: i32,
    pub attacks_used: i32,
    /// Whether the WML `defense_weight` allows this weapon to retaliate.
    pub can_retaliate: bool,
    /// The first `[specials]` tag, retained for the combat rules to interpret.
    pub specials: Option<Node>,
}

impl WeaponProfile {
    /// Consumes the contents of one `[attack]` tag.
    pub fn from_wml(attack: Node) -> Self {
        let text = |key| attack.attribute(key).unwrap_or_default().to_owned();
        let int = |key, default| {
            attack
                .attributes
                .get(key)
                .filter(|value| value.translations.is_empty())
                .and_then(|value| value.text.trim().parse().ok())
                .unwrap_or(default)
        };
        Self {
            id: text("name"),
            damage_type: text("type"),
            range: text("range"),
            min_range: int("min_range", 1),
            max_range: int("max_range", 1),
            damage: int("damage", 0),
            strikes: int("number", 0),
            accuracy: int("accuracy", 0),
            parry: int("parry", 0),
            // The native default spends all movement for standard units.
            movement_used: int("movement_used", 100_000),
            attacks_used: int("attacks_used", 1),
            can_retaliate: attack
                .attributes
                .get("defense_weight")
                .filter(|value| value.translations.is_empty())
                .and_then(|value| value.text.trim().parse::<f64>().ok())
                .is_none_or(|weight| weight > 0.0),
            specials: attack
                .children
                .into_iter()
                .find_map(|(tag, child)| (tag == "specials").then_some(child)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    #[test]
    fn reads_rule_fields_and_the_first_specials_child() {
        let document = wml::parse(
            r#"
            [attack]
                name=fire_arrow
                type=fire
                range=ranged
                min_range=2
                max_range=3
                damage=6
                number=4
                accuracy=10
                parry=-5
                movement_used=0
                attacks_used=2
                [specials]
                    [chance_to_hit]
                        id=marksman
                    [/chance_to_hit]
                [/specials]
                [specials]
                    [firststrike]
                    [/firststrike]
                [/specials]
            [/attack]
            "#,
        )
        .unwrap();
        let (_, attack) = document.children.into_iter().next().unwrap();
        let weapon = WeaponProfile::from_wml(attack);
        assert_eq!(weapon.id, "fire_arrow");
        assert_eq!(weapon.damage_type, "fire");
        assert_eq!(weapon.range, "ranged");
        assert_eq!((weapon.min_range, weapon.max_range), (2, 3));
        assert_eq!((weapon.damage, weapon.strikes), (6, 4));
        assert_eq!((weapon.accuracy, weapon.parry), (10, -5));
        assert_eq!((weapon.movement_used, weapon.attacks_used), (0, 2));
        let specials = weapon.specials.unwrap();
        assert_eq!(specials.children.len(), 1);
        let (kind, definition) = &specials.children[0];
        assert_eq!(kind, "chance_to_hit");
        assert_eq!(definition.attribute("id"), Some("marksman"));
    }

    #[test]
    fn uses_native_defaults_for_empty_fields() {
        let attack = wml::parse("movement_used=\nattacks_used=\"\"").unwrap();
        let weapon = WeaponProfile::from_wml(attack);
        assert!(weapon.id.is_empty());
        assert!(weapon.damage_type.is_empty());
        assert!(weapon.range.is_empty());
        assert_eq!((weapon.min_range, weapon.max_range), (1, 1));
        assert_eq!((weapon.damage, weapon.strikes), (0, 0));
        assert_eq!((weapon.accuracy, weapon.parry), (0, 0));
        assert_eq!((weapon.movement_used, weapon.attacks_used), (100_000, 1));
        assert!(weapon.specials.is_none());
    }
}

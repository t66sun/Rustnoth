use crate::wml::Node;

const MAGICAL_HIT_CHANCE: u8 = 70;

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

    pub(crate) fn has_firststrike(&self) -> bool {
        self.specials.as_ref().is_some_and(|specials| {
            specials.children.iter().any(|(tag, definition)| {
                is_unconditional_firststrike(tag, definition)
            })
        })
    }

    pub(crate) fn magical_hit_chance(&self) -> Option<u8> {
        self.specials.as_ref().and_then(|specials| {
            specials
                .children
                .iter()
                .any(|(tag, definition)| is_unconditional_magical(tag, definition))
                .then_some(MAGICAL_HIT_CHANCE)
        })
    }

    pub(crate) fn supports_specials(&self) -> bool {
        self.specials.as_ref().is_none_or(|specials| {
            specials.children.iter().all(|(tag, definition)| {
                is_unconditional_firststrike(tag, definition)
                    || is_unconditional_magical(tag, definition)
            })
        })
    }
}

fn is_unconditional_magical(tag: &str, definition: &Node) -> bool {
    tag == "chance_to_hit"
        && definition.children.is_empty()
        && definition.attribute("id") == Some("magical")
        && definition.attribute("value").and_then(|value| value.parse().ok())
            == Some(MAGICAL_HIT_CHANCE)
        && definition.attribute("cumulative") == Some("no")
        && definition.attributes.iter().all(|(key, value)| match key.as_str() {
            "id" | "value" | "cumulative" => value.translations.is_empty(),
            "name" | "description" | "special_note" => true,
            _ => false,
        })
}

fn is_unconditional_firststrike(tag: &str, definition: &Node) -> bool {
    tag == "firststrike"
        && definition.children.is_empty()
        && definition.attributes.keys().all(|key| {
            matches!(key.as_str(), "id" | "name" | "description" | "special_note")
        })
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

    #[test]
    fn accepts_unconditional_firststrike_and_rejects_unsupported_specials() {
        let weapon = |special| {
            WeaponProfile::from_wml(
                wml::parse(format!("[specials]\n{special}\n[/specials]")).unwrap(),
            )
        };
        let stock = weapon(
            "[firststrike]\nid=firststrike\nname=_\"first strike\"\n\
             description=_\"Strikes first.\"\nspecial_note=_\"First strike.\"\n[/firststrike]",
        );
        assert!(stock.has_firststrike());
        assert!(stock.supports_specials());
        for unsupported in [
            "[firststrike]\nid=firststrike\nactive_on=defense\n[/firststrike]",
            "[firststrike]\n[filter_opponent]\ntype=Fighter\n[/filter_opponent]\n[/firststrike]",
            "[poison]\nid=poison\n[/poison]",
        ] {
            let weapon = weapon(unsupported);
            assert!(!weapon.has_firststrike());
            assert!(!weapon.supports_specials());
        }
    }

    #[test]
    fn recognizes_stock_magical_without_accepting_other_hit_chance_effects() {
        let weapon = |fields| {
            WeaponProfile::from_wml(
                wml::parse(format!(
                    "[specials]\n[chance_to_hit]\n{fields}\n[/chance_to_hit]\n[/specials]"
                ))
                .unwrap(),
            )
        };
        let stock = weapon(
            "id=magical\nvalue=70\ncumulative=no\nname= _ \"magical\"\n\
             description= _ \"Sets the chance to hit.\"\nspecial_note= _ \"Magical attacks.\"",
        );
        assert!(stock.supports_specials());
        assert_eq!(stock.magical_hit_chance(), Some(70));

        for fields in [
            "id=magical\nvalue=70\ncumulative=no\nactive_on=offense",
            "id=magical\nvalue=70\ncumulative=no\n[filter_opponent]\ntype=Fighter\n[/filter_opponent]",
            "id=magical\nvalue=70\ncumulative=yes",
            "id=magical\nvalue=80\ncumulative=no",
            "id=marksman\nvalue=70\ncumulative=no",
        ] {
            let unsupported = weapon(fields);
            assert!(!unsupported.supports_specials());
            assert_eq!(unsupported.magical_hit_chance(), None);
        }
    }
}

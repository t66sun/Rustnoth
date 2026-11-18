use std::num::NonZeroU32;

use crate::wml::Node;

const MAGICAL_HIT_CHANCE: u8 = 70;
const MARKSMAN_HIT_CHANCE: u8 = 60;
const BERSERK_ROUNDS: NonZeroU32 = NonZeroU32::new(30).unwrap();

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

    fn magical_hit_chance(&self) -> Option<u8> {
        self.specials.as_ref().and_then(|specials| {
            specials
                .children
                .iter()
                .any(|(tag, definition)| is_unconditional_magical(tag, definition))
                .then_some(MAGICAL_HIT_CHANCE)
        })
    }

    pub(crate) fn hit_chance(&self, base: u8, attacking: bool) -> u8 {
        let chance = self.magical_hit_chance().unwrap_or(base);
        if attacking && self.specials.as_ref().is_some_and(|specials| {
            specials.children.iter().any(|(tag, definition)| is_stock_marksman(tag, definition))
        }) {
            // Cumulative [chance_to_hit] retains the base even alongside a fixed value.
            chance.max(base).max(MARKSMAN_HIT_CHANCE)
        } else {
            chance
        }
    }

    pub(crate) fn has_charge(&self) -> bool {
        self.specials.as_ref().is_some_and(|specials| {
            specials.children.iter().any(|(tag, definition)| is_stock_charge(tag, definition))
        })
    }

    pub(crate) fn has_poison(&self) -> bool {
        self.has_status_special("poison")
    }

    pub(crate) fn combat_rounds(&self) -> NonZeroU32 {
        if self.specials.as_ref().is_some_and(|specials| {
            specials.children.iter().any(|(tag, definition)| is_stock_berserk(tag, definition))
        }) {
            BERSERK_ROUNDS
        } else {
            NonZeroU32::MIN
        }
    }

    pub(crate) fn has_drains(&self) -> bool {
        self.has_status_special("drains")
    }

    pub(crate) fn has_slow(&self) -> bool {
        self.has_status_special("slow")
    }

    fn has_status_special(&self, name: &str) -> bool {
        self.specials.as_ref().is_some_and(|specials| {
            specials.children.iter().any(|(tag, cfg)| tag == name && is_status_special(tag, cfg))
        })
    }

    pub(crate) fn supports_specials(&self) -> bool {
        self.specials.as_ref().is_none_or(|specials| {
            specials.children.iter().all(|(tag, definition)| {
                is_unconditional_firststrike(tag, definition)
                    || is_stock_berserk(tag, definition)
                    || is_unconditional_magical(tag, definition)
                    || is_stock_marksman(tag, definition)
                    || is_stock_charge(tag, definition)
                    || (matches!(tag.as_str(), "drains" | "slow" | "poison") && is_status_special(tag, definition))
            })
        })
    }
}

fn is_stock_marksman(tag: &str, definition: &Node) -> bool {
    tag == "chance_to_hit"
        && definition.children.is_empty()
        && definition.attribute("id") == Some("marksman")
        && definition.attribute("value").and_then(|value| value.parse().ok())
            == Some(MARKSMAN_HIT_CHANCE)
        && definition.attribute("cumulative") == Some("yes")
        && definition.attribute("active_on") == Some("offense")
        && definition.attributes.iter().all(|(key, value)| match key.as_str() {
            "id" | "value" | "cumulative" | "active_on" => value.translations.is_empty(),
            "name" | "description" | "special_note" => true,
            _ => false,
        })
}

fn is_stock_berserk(tag: &str, definition: &Node) -> bool {
    tag == "berserk"
        && definition.children.is_empty()
        && definition.attribute("id") == Some("berserk")
        && definition.attribute("value").and_then(|value| value.parse().ok()) == Some(BERSERK_ROUNDS)
        && definition.attributes.iter().all(|(key, value)| match key.as_str() {
            "id" | "value" => value.translations.is_empty(),
            "name" | "description" | "special_note" => true,
            _ => false,
        })
}

fn is_status_special(tag: &str, definition: &Node) -> bool {
    definition.attribute("id") == Some(tag)
        && definition.children.is_empty()
        && definition.attributes.iter().all(|(key, value)| match key.as_str() {
            "id" => value.translations.is_empty(),
            "name" | "description" | "special_note" => true,
            _ => false,
        })
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

fn is_stock_charge(tag: &str, definition: &Node) -> bool {
    tag == "damage"
        && definition.children.is_empty()
        && definition.attribute("id") == Some("charge")
        && definition.attribute("multiply") == Some("2")
        && definition.attribute("apply_to") == Some("both")
        && definition.attribute("active_on") == Some("offense")
        && definition.attributes.keys().all(|key| {
            matches!(
                key.as_str(),
                "id" | "multiply" | "apply_to" | "active_on" | "name" | "description" | "special_note"
            )
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
    fn rejects_berserk_with_a_different_limit_or_trigger() {
        for fields in [
            "id=berserk\nvalue=40",
            "id=berserk\nvalue=30\nactive_on=offense",
            "id=berserk\nvalue=30\n[filter_opponent]\nlevel=1\n[/filter_opponent]",
        ] {
            let weapon = WeaponProfile::from_wml(
                wml::parse(format!("[specials]\n[berserk]\n{fields}\n[/berserk]\n[/specials]")).unwrap(),
            );
            assert!(!weapon.supports_specials());
        }
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

    #[test]
    fn stock_marksman_sets_an_offensive_floor_and_rejects_modified_specials() {
        let fields = "id=marksman\nvalue=60\ncumulative=yes\nactive_on=offense";
        let weapon = |fields: &str| {
            WeaponProfile::from_wml(wml::parse(format!(
                "[specials]\n[chance_to_hit]\n{fields}\n[/chance_to_hit]\n[/specials]"
            )).unwrap())
        };
        let stock = weapon(fields);
        assert!(stock.supports_specials());
        assert_eq!(stock.hit_chance(30, true), 60);
        assert_eq!(stock.hit_chance(90, true), 90);
        assert_eq!(stock.hit_chance(30, false), 30);
        for modified in [
            fields.replace("60", "70"),
            fields.replace("offense", "defense"),
            fields.replace("yes", "no"),
            format!("{fields}\n[filter_opponent]\ntype=Fighter\n[/filter_opponent]"),
        ] {
            assert!(!weapon(&modified).supports_specials());
        }
    }

    #[test]
    fn accepts_stock_drains_and_rejects_modified_drain_effects() {
        let weapon = |fields| {
            WeaponProfile::from_wml(
                wml::parse(format!(
                    "[specials]\n[drains]\nid=drains\n{fields}\n[/drains]\n[/specials]"
                ))
                .unwrap(),
            )
        };
        let stock = weapon("name=_\"drains\"");
        assert!(stock.has_drains());
        assert!(stock.supports_specials());
        for fields in [
            "value=25",
            "active_on=defense",
            "[filter_opponent]\ntype=Human\n[/filter_opponent]",
        ] {
            let modified = weapon(fields);
            assert!(!modified.has_drains());
            assert!(!modified.supports_specials());
        }
    }

    #[test]
    fn accepts_stock_charge_and_rejects_modified_damage_rules() {
        let charge = "[damage]\nid=charge\nmultiply=2\napply_to=both\nactive_on=offense\n[/damage]";
        let weapon = |special: &str| {
            WeaponProfile::from_wml(
                wml::parse(format!("[specials]\n{special}\n[/specials]")).unwrap(),
            )
        };
        let stock = weapon(charge);
        assert!(stock.has_charge());
        assert!(stock.supports_specials());
        for modified in [
            charge.replace("multiply=2", "multiply=3"),
            charge.replace("active_on=offense", "active_on=defense"),
            charge.replace(
                "[/damage]",
                "[filter_opponent]\ntype=Fighter\n[/filter_opponent]\n[/damage]",
            ),
        ] {
            let weapon = weapon(&modified);
            assert!(!weapon.has_charge());
            assert!(!weapon.supports_specials());
        }
    }
}

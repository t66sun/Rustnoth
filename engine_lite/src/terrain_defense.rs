use std::collections::BTreeMap;

use crate::terrain_aliases::{AliasKind, AliasTerm, TerrainAliases};
use crate::wml::Node;

const MAX_ALIAS_DEPTH: usize = 100;

/// A movetype's terrain chance to be hit, keyed by WML terrain id.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TerrainDefense {
    entries: BTreeMap<String, HitChances>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HitChances {
    ordinary: u8,
    floor: u8,
}

impl HitChances {
    const DEFAULT: Self = Self {
        ordinary: 100,
        floor: 0,
    };

    fn from_wml(value: i32) -> Self {
        let ordinary = value.unsigned_abs().min(100) as u8;
        let floor = if value < 0 { ordinary } else { 0 };
        Self { ordinary, floor }
    }

    fn choose(self, other: Self, prefer_best: bool) -> Self {
        if prefer_best {
            Self {
                ordinary: self.ordinary.min(other.ordinary),
                floor: self.floor.max(other.floor),
            }
        } else {
            Self {
                ordinary: self.ordinary.max(other.ordinary),
                floor: self.floor.min(other.floor),
            }
        }
    }
}

impl TerrainDefense {
    /// Read the contents of a preprocessed `[defense]` tag.
    pub fn from_wml(table: &Node) -> Self {
        Self::from_wml_tables([table])
    }

    /// Read tables in declaration order, with the last value for each terrain ID.
    pub(crate) fn from_wml_tables<'a>(tables: impl IntoIterator<Item = &'a Node>) -> Self {
        let mut entries = BTreeMap::new();
        for table in tables {
            for (id, value) in &table.attributes {
                let chances = if value.translations.is_empty() {
                    value
                        .text
                        .trim()
                        .parse()
                        .map_or(HitChances::DEFAULT, HitChances::from_wml)
                } else {
                    HitChances::DEFAULT
                };
                entries.insert(id.clone(), chances);
            }
        }
        Self { entries }
    }

    /// Return the terrain's percent chance to be hit before weapon modifiers.
    ///
    /// Negative entries cap defense by imposing a minimum chance to be hit.
    pub fn hit_chance(&self, aliases: &TerrainAliases, code: &str) -> u8 {
        self.hit_chance_with_fallback(aliases, code, None)
    }

    /// Fill absent terrain IDs from the inherited table before evaluating aliases.
    pub(crate) fn hit_chance_with_fallback(
        &self,
        aliases: &TerrainAliases,
        code: &str,
        inherited: Option<&Self>,
    ) -> u8 {
        let chances = self.resolve(aliases, code, &mut Vec::new(), inherited);
        chances.ordinary.max(chances.floor)
    }

    fn resolve<'a>(
        &self,
        aliases: &'a TerrainAliases,
        code: &'a str,
        ancestors: &mut Vec<&'a str>,
        inherited: Option<&Self>,
    ) -> HitChances {
        if ancestors.len() > MAX_ALIAS_DEPTH || ancestors.contains(&code) {
            return HitChances::DEFAULT;
        }

        let terms = aliases.terms(code, AliasKind::Defense);
        if terms.is_empty() || terms.as_slice() == [AliasTerm::Terrain(code)] {
            return aliases
                .terrain_id(code)
                .and_then(|id| self.entries.get(id).or_else(|| inherited?.entries.get(id)))
                .copied()
                .unwrap_or(HitChances::DEFAULT);
        }

        let mut result = if terms.first() == Some(&AliasTerm::Worst) {
            HitChances {
                ordinary: 0,
                floor: 100,
            }
        } else {
            HitChances::DEFAULT
        };
        // The alias sequence chooses ordinary odds and defense caps separately.
        let mut prefer_best = true;
        ancestors.push(code);
        for term in terms {
            match term {
                AliasTerm::Best => prefer_best = true,
                AliasTerm::Worst => prefer_best = false,
                AliasTerm::Terrain(terrain) => {
                    let candidate = self.resolve(aliases, terrain, ancestors, inherited);
                    result = result.choose(candidate, prefer_best);
                }
            }
        }
        ancestors.pop();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        board::AttackChoice, fixed_match::Action, hex::Hex, scenario_bundle::Bundle, wml,
    };

    fn catalog(config: &str) -> TerrainAliases {
        TerrainAliases::from_wml(&wml::parse(config).unwrap())
    }

    fn defense(config: &str) -> TerrainDefense {
        TerrainDefense::from_wml(&wml::parse(config).unwrap())
    }

    #[test]
    fn reads_hit_percentages_by_terrain_id_and_defaults_to_one_hundred() {
        let terrain = catalog(
            "[terrain_type]\nstring=Gg\nid=flat\n[/terrain_type]\n\
             [terrain_type]\nstring=Ww\nid=shallow_water\n[/terrain_type]\n\
             [terrain_type]\nstring=Mm\nid=mountains\n[/terrain_type]",
        );
        let odds = defense("flat=40\nshallow_water=120\n");
        assert_eq!(odds.hit_chance(&terrain, "Gg"), 40);
        assert_eq!(odds.hit_chance(&terrain, "Ww"), 100);
        assert_eq!(odds.hit_chance(&terrain, "Mm"), 100);
        assert_eq!(odds.hit_chance(&terrain, "Qq"), 100);
    }

    #[test]
    fn defense_caps_survive_a_best_alias_but_can_drop_through_a_worst_alias() {
        let terrain = catalog(
            "[terrain_type]\nstring=Gg\nid=flat\n[/terrain_type]\n\
             [terrain_type]\nstring=Ft\nid=forest\n[/terrain_type]\n\
             [terrain_type]\nstring=^Fp\ndef_alias=_bas,Ft\n[/terrain_type]\n\
             [terrain_type]\nstring=Q1\ndef_alias=\"-,Ft,Gg,+,Gg\"\n[/terrain_type]",
        );
        let uncapped = defense("flat=40\nforest=70");
        let capped = defense("flat=40\nforest=-70");
        assert_eq!(uncapped.hit_chance(&terrain, "Gg^Fp"), 40);
        assert_eq!(capped.hit_chance(&terrain, "Gg^Fp"), 70);
        assert_eq!(capped.hit_chance(&terrain, "Ft"), 70);
        assert_eq!(capped.hit_chance(&terrain, "Q1"), 40);
    }

    #[test]
    fn combat_uses_unit_defense_and_inherits_other_terrain_keys() {
        let bundle = Bundle::parse(
            br#"
            [terrain_type]
                string=Gg
                id=flat
            [/terrain_type]
            [terrain_type]
                string=Ft
                id=forest
            [/terrain_type]
            [terrain_type]
                string=^Fp
                def_alias=_bas,Ft
            [/terrain_type]
            [units]
                [movetype]
                    name=foot
                    [defense]
                        flat=100
                        forest=-70
                    [/defense]
                [/movetype]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    movement_type=foot
                    [defense]
                        flat=50
                    [/defense]
                    [defense]
                        flat=0
                    [/defense]
                    [attack]
                        name=sword
                        type=blade
                        range=melee
                        damage=5
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg^Fp,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        game.step(Action::Attack(AttackChoice {
            from: Hex { x: 0, y: 0 },
            to: Hex { x: 1, y: 0 },
            weapon: 0,
        }))
        .unwrap();

        let snapshot = game.audit_snapshot();
        assert_eq!((snapshot.units[0].hitpoints, snapshot.units[1].hitpoints), (5, 10));
    }
}

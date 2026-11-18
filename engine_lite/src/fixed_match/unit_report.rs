//! Unit information shown in the battlefield sidebar for one viewing side.
use super::{FixedMatch, Hex};
use crate::{strike_damage::strike_damage, time_of_day::Alignment, visibility::HexVisibility};

pub struct UnitReport {
    pub name: Option<String>,
    pub level: i32,
    pub alignment: Alignment,
    pub alignment_bonus: i64,
    pub max_movement: i32,
    pub defense: u8,
    pub traits: Vec<String>,
    pub weapons: Vec<WeaponReport>,
}

pub struct WeaponReport {
    pub id: String,
    pub damage_type: String,
    pub range: String,
    pub damage: i64,
    pub strikes: u32,
    pub specials: Vec<String>,
}

impl FixedMatch<'_> {
    /// Display the unit at `position`. Known terrain under the pointer affects
    /// defense, time of day and the adjacent providers used for weapon reports.
    pub fn unit_report(&self, side: usize, position: Hex, pointer: Hex) -> Option<UnitReport> {
        self.scenario.sides.get(side.checked_sub(1)?)?;
        let vision = self.effective_vision(side);
        let board = &self.scenario.board;
        let visible =
            |hex| vision.at(hex) == Some(HexVisibility::Visible) && self.unit_visible_on_known_hex(side, hex);
        let unit = board.unit_at(position)?;
        if unit.side != side && !visible(position) {
            return None;
        }
        let preview = match vision.at(pointer) {
            Some(HexVisibility::Visible | HexVisibility::Fogged) => pointer,
            _ => position,
        };
        let kind = unit.unit_type;
        let inherited = self
            .rules
            .movement
            .get(&kind.movement_type)
            .map(|profile| &profile.defense);
        let defense = 100
            - kind.defense_overrides.hit_chance_with_fallback(
                self.rules.terrain,
                board.map().terrain_at(preview)?,
                inherited,
            );
        let lawful = self.lawful_bonus_in_view(self.current_round, preview, visible);
        let alignment_bonus =
            self.scenario
                .schedule
                .combat_modifier(lawful, kind.alignment, unit.is_fearless());
        let leadership = self.leadership_in_view(preview, unit, visible);
        let multiplier = ((100 + alignment_bonus + leadership) * 100)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        let weapons = kind
            .attacks
            .iter()
            .enumerate()
            .map(|(index, weapon)| WeaponReport {
                id: weapon.id.clone(),
                damage_type: weapon.damage_type.clone(),
                range: weapon.range.clone(),
                damage: strike_damage(unit.attack_damage(index), multiplier, unit.slowed),
                strikes: unit.attack_strikes(index),
                specials: weapon
                    .specials
                    .iter()
                    .flat_map(|s| &s.children)
                    .filter_map(|(_, spec)| spec.attribute("name"))
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
            .collect();
        Some(UnitReport {
            name: unit.display_name.clone(),
            level: kind.level,
            alignment: kind.alignment,
            alignment_bonus,
            max_movement: unit.max_movement(),
            defense,
            traits: unit
                .traits()
                .filter_map(|t| t.attribute("name"))
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            weapons,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario_bundle::Bundle;

    #[test]
    fn sidebar_uses_authored_name_current_damage_and_viewer_visibility() {
        let data = br#"
[units]
[unit_type]
id=Scout
level=1
hitpoints=30
movement=1
alignment=lawful
[attack]
name=bow
range=ranged
type=pierce
damage=8
number=3
[/attack]
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[time]
id=day
lawful_bonus=25
[/time]
[side]
side=1
type=Scout
name=Deoran
fog=yes
shroud=yes
random_traits=no
[/side]
[side]
side=2
type=Scout
random_traits=no
[/side]
[/scenario]"#;
        let bundle = Bundle::parse(data).unwrap();
        let game = bundle.start(1, |_| unreachable!()).unwrap();
        let home = Hex { x: 0, y: 0 };
        let distant = Hex { x: 5, y: 0 };
        let report = game.unit_report(1, home, distant).unwrap();
        assert_eq!(report.name.as_deref(), Some("Deoran"));
        assert_eq!(report.level, 1);
        assert_eq!((report.weapons[0].damage, report.weapons[0].strikes), (10, 3));
        assert!(game.unit_report(1, distant, home).is_none());
    }
}

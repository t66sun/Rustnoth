use super::FixedMatch;
use crate::{hex::Hex, time_of_day::{Alignment, TimeOfDay}, visibility::HexVisibility};

/// The schedule the viewer can see, plus the alignment modifiers for its current slot.
pub struct TimeReport<'a> {
    pub current: usize,
    pub schedule: &'a [TimeOfDay],
    pub lawful: i32,
    pub liminal: i64,
}

impl FixedMatch<'_> {
    pub fn time_report(&self, viewer: usize, position: Option<Hex>) -> Option<TimeReport<'_>> {
        let side = self.scenario.sides.get(viewer.checked_sub(1)?)?;
        if self.global_time_of_day().is_none() && self.linger_time().is_none() { return None; }
        let shared = position.filter(|_| side.fog || side.shroud).map(|_| self.effective_vision(viewer));
        let vision = shared.as_ref().unwrap_or(&self.vision[viewer - 1]);
        let known = position.filter(|&at| matches!(vision.at(at), Some(HexVisibility::Visible | HexVisibility::Fogged)));
        let schedule = &self.scenario.schedule;
        let (current, entries) = schedule.cycle_report(self.current_round, known);
        let lawful = match known.filter(|&at| vision.at(at) == Some(HexVisibility::Visible)) {
            Some(at) => self.lawful_bonus_in_view(self.current_round, at, |hex|
                vision.at(hex) == Some(HexVisibility::Visible) && self.unit_visible_on_known_hex(viewer, hex)),
            None => entries[current].lawful_bonus,
        };
        Some(TimeReport { current, schedule: entries, lawful,
            liminal: schedule.combat_modifier(lawful, Alignment::Liminal, false) })
    }

    pub(super) fn lawful_bonus_at(&self, round: i32, position: Hex) -> i32 {
        self.lawful_bonus_in_view(round, position, |_| true)
    }

    pub(super) fn lawful_bonus_in_view(&self, round: i32, position: Hex, visible: impl Fn(Hex) -> bool) -> i32 {
        let board = &self.scenario.board;
        let scheduled = self.scenario.schedule.at_hex(round, position).lawful_bonus;
        let base = board.map().terrain_at(position).map_or(scheduled, |code|
            self.rules.terrain.light_bonus(code, scheduled)
        );
        if base >= 25 {
            return base;
        }
        let illuminated = std::iter::once(position).chain(position.neighbors())
            .filter(|hex| visible(*hex))
            .any(|hex| board.unit_at(hex).is_some_and(|unit| unit.unit_type.illuminates));
        if illuminated { (base + 25).min(25) } else { base }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::AttackChoice, fixed_match::Action, scenario_bundle::Bundle};

    #[test]
    fn a_shrouded_clock_cannot_reveal_an_unexplored_time_area() {
        let bundle = Bundle::parse(br#"
            [units]
                [unit_type]
                    id=Scout
                    hitpoints=25
                    movement=1
                    vision=1
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
                [time]
                    id=dawn
                    name=Dawn
                    image=custom-dawn.png
                [/time]
                [time_area]
                    x,y=8,1
                    [time]
                        id=cavern
                    [/time]
                [/time_area]
                [side]
                    type=Scout
                    shroud=yes
                [/side]
                [side]
                    type=Scout
                [/side]
            [/scenario]"#).unwrap();
        let game = bundle.start(19, |_| unreachable!()).unwrap();
        let cavern = Hex { x: 7, y: 0 };
        let player = game.time_report(1, Some(cavern)).unwrap();
        let occupant = game.time_report(2, Some(cavern)).unwrap();
        assert_eq!((&*player.schedule[0].id, &*player.schedule[0].image), ("dawn", "custom-dawn.png"));
        assert_eq!(occupant.schedule[0].id, "cavern");
    }

    #[test]
    fn an_attacker_in_a_daytime_area_uses_daylight_against_a_defender_outside_it() {
        let bundle = Bundle::parse(br#"
            [units]
                [unit_type]
                    id=Fighter
                    alignment=lawful
                    hitpoints=40
                    [attack]
                        range=melee
                        damage=8
                        number=1
                        accuracy=100
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [time]
                    lawful_bonus=-25
                [/time]
                [time_area]
                    x=1
                    y=1
                    [time]
                        lawful_bonus=25
                    [/time]
                [/time_area]
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
                [/side]
            [/scenario]"#).unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        game.step(Action::Attack(AttackChoice { from, to, weapon: 0 })).unwrap();
        assert_eq!(game.scenario.board.unit_at(to).unwrap().hitpoints, 30);
        assert_eq!(game.scenario.board.unit_at(from).unwrap().hitpoints, 34);
    }

    #[test]
    fn beam_of_light_changes_a_night_attack_and_lava_caps_daylight() {
        let config = br#"
            [terrain_type]
                string=Gg
            [/terrain_type]
            [terrain_type]
                string=^Ii
                light=25
            [/terrain_type]
            [terrain_type]
                string=Ql
                light=25
                max_light=35
            [/terrain_type]
            [units]
                [unit_type]
                    id=Fighter
                    alignment=lawful
                    hitpoints=30
                    [attack]
                        range=melee
                        damage=10
                        number=1
                        accuracy=100
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg^Ii,2 Gg,Ql,Xv
Xv,Xv,Xv,Xv,Xv"
                [time]
                    id=night
                    lawful_bonus=-25
                [/time]
                [time]
                    id=day
                    lawful_bonus=25
                [/time]
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
                [/side]
            [/scenario]"#;
        let bundle = Bundle::parse(config).unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        assert_eq!(game.lawful_bonus_at(1, from), 0);
        assert_eq!(game.lawful_bonus_at(1, to), -25);
        assert_eq!(game.lawful_bonus_at(2, Hex { x: 2, y: 0 }), 35);
        game.step(Action::Attack(AttackChoice { from, to, weapon: 0 })).unwrap();
        assert_eq!(game.scenario.board.unit_at(to).unwrap().hitpoints, 20);
    }

    #[test]
    fn illumination_applies_once_at_each_striker_and_to_nightstalk() {
        for (base, lit, attack_damage, retaliation_damage) in [
            (-50, -25, 6, 4),
            (-25, 0, 8, 6),
            (0, 25, 10, 8),
            (25, 25, 10, 10),
            (50, 50, 12, 12),
        ] {
            let config = format!(r#"
                [units]
                    [unit_type]
                        id=Fighter
                        alignment=lawful
                        hitpoints=40
                        [attack]
                            range=melee
                            damage=8
                            number=1
                            accuracy=100
                        [/attack]
                    [/unit_type]
                    [unit_type]
                        id=Lamp
                        [abilities]
                            [illuminates]
                                id=illumination
                                value=25
                                max_value=25
                                cumulative=no
                                affect_self=yes
                            [/illuminates]
                        [/abilities]
                    [/unit_type]
                    [unit_type]
                        id=Shade
                        [abilities]
                            [hides]
                                id=nightstalk
                                [filter]
                                    [filter_location]
                                        time_of_day=chaotic
                                    [/filter_location]
                                [/filter]
                            [/hides]
                        [/abilities]
                    [/unit_type]
                [/units]
                [scenario]
                    map_data="Xv,Xv,Xv,Xv,Xv
Xv,Gg,1 Gg,2 Gg,Xv
Xv,Gg,Gg,Gg,Xv
Xv,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv"
                    [time]
                        id=light
                        lawful_bonus={base}
                    [/time]
                    [side]
                        side=1
                        type=Fighter
                    [/side]
                    [side]
                        side=2
                        type=Fighter
                        [unit]
                            type=Lamp
                            x,y=1,1
                        [/unit]
                        [unit]
                            type=Lamp
                            x,y=1,2
                        [/unit]
                        [unit]
                            type=Shade
                            x,y=1,3
                        [/unit]
                    [/side]
                [/scenario]"#);
            let bundle = Bundle::parse(config.as_bytes()).unwrap();
            let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
            let from = Hex { x: 1, y: 0 };
            let to = Hex { x: 2, y: 0 };
            assert_eq!(game.lawful_bonus_at(1, Hex { x: 0, y: 0 }), lit);
            assert_eq!(game.lawful_bonus_at(1, from), lit);
            assert_eq!(game.lawful_bonus_at(1, to), base);
            let view = game.observation_for(1).unwrap();
            assert_eq!(view.units.iter().any(|unit| unit.type_id == "Shade"), lit >= 0);
            game.step(Action::Attack(AttackChoice { from, to, weapon: 0 })).unwrap();
            assert_eq!(game.scenario.board.unit_at(to).unwrap().hitpoints, 40 - attack_damage);
            assert_eq!(game.scenario.board.unit_at(from).unwrap().hitpoints, 40 - retaliation_damage);
        }
    }
}

#[cfg(test)]
mod regional_event_tests {
    use crate::{fixed_match::Action, hex::Hex, scenario_bundle::Bundle};

    #[test]
    fn turn_events_place_and_remove_regional_schedules_by_id() {
        let text = br#"[units]
[unit_type]
id=Guard
hitpoints=24
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[/side]
[time]
id=global
[/time]
[time_area]
id=old
x,y=1,1
[time]
id=old_light
[/time]
[/time_area]
[event]
name=side 2 turn
[time_area]
id=torch
x,y=1,1
current_time=1
[time]
id=smoke
[/time]
[time]
id=fire
[/time]
[/time_area]
[/event]
[event]
name=turn 2
[remove_time_area]
id=old,torch
[/remove_time_area]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(text).unwrap();
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let west = Hex { x: 0, y: 0 };
        let east = Hex { x: 1, y: 0 };
        assert_eq!(game.scenario.schedule.at_hex(1, west).id, "old_light");
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.scenario.schedule.at_hex(1, west).id, "fire");
        assert_eq!(game.scenario.schedule.at_hex(1, east).id, "global");
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.scenario.schedule.at_hex(2, west).id, "global");
    }
}

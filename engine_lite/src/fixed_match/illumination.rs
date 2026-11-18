use super::FixedMatch;
use crate::hex::Hex;

impl FixedMatch<'_> {
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

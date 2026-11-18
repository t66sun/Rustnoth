use super::{FixedMatch, MatchError};
use crate::{hex::Hex, scenario_events::kill::KillAction};

impl FixedMatch<'_> {
    pub(super) fn kill_units(&mut self, action: KillAction, primary: Option<Hex>) -> Result<bool, MatchError> {
        let board = &self.scenario.board;
        let killer = board.units().filter(|(pos, unit)| action.matches_killer(*pos, unit))
            .min_by_key(|(_, unit)| unit.serial).map(|(pos, _)| pos);
        let mut victims: Vec<_> = board.units().filter(|(pos, unit)| action.matches_map(*pos, unit, primary))
            .map(|(position, unit)| (unit.serial, position, unit.side)).collect();
        victims.sort_unstable_by_key(|&(serial, _, _)| serial);
        let targeted = !victims.is_empty();
        for (serial, position, side) in victims {
            if self.scenario.board.unit_at(position).is_none_or(|unit| unit.serial != serial) { continue; }
            if action.fire_event && self.kill_stack.iter().filter(|&&hex| hex == position).count() < 9 {
                self.kill_stack.push(position);
                let secondary = Some(killer.unwrap_or(position));
                let result = self.fire_units_event("last_breath", position, secondary)
                    .and_then(|()| self.fire_units_event("die", position, secondary));
                self.kill_stack.pop();
                result?;
            }
            // Nested events may already have erased this unit and placed another at its hex.
            if self.scenario.board.unit_at(position).is_some_and(|unit| unit.serial == serial) {
                self.scenario.board.remove_casualty(position);
                self.refog(side);
            }
        }
        self.scenario.board.retain_reserves(|unit| !action.matches_recall(unit));
        Ok(targeted)
    }
}

#[cfg(test)]
mod tests {
    use crate::{fixed_match::{Action, MatchState}, scenario_bundle::Bundle};

    fn scenario(events: &str) -> Bundle {
        let source = format!(r#"[units]
[unit_type]
id=Guard
hitpoints=24
movement=5
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
id=West
type=Guard
[unit]
id=Veteran
type=Guard
placement=recall
[/unit]
[/side]
[side]
id=East
type=Guard
[unit]
id=Victim
type=Guard
x,y=2,1
[/unit]
[/side]
{events}
[/scenario]"#);
        Bundle::parse(source.as_bytes()).unwrap()
    }

    #[test]
    fn scripted_kills_fire_ordered_death_events_and_clear_matching_reserves() {
        let bundle = scenario(r#"[event]
name=start
[kill]
id=Victim
fire_event=yes
[secondary_unit]
id=West
[/secondary_unit]
[/kill]
[gold]
side=1
amount=5
[/gold]
[kill]
id=Veteran
x,y=recall,recall
[/kill]
[/event]
[event]
name=last breath
[filter]
id=Victim
[/filter]
[filter_second]
id=West
[/filter_second]
[modify_side]
side=1
gold=7
[/modify_side]
[/event]
[event]
name=die
[filter_second]
id=West
[/filter_second]
[gold]
side=1
amount=3
[/gold]
[/event]
[event]
name=side 2 turn
[kill]
id=East
[/kill]
[/event]"#);
        let mut game = bundle.start(4, |_| unreachable!()).unwrap();
        let snapshot = game.audit_snapshot();
        assert_eq!(snapshot.gold[0], 15);
        assert!(snapshot.reserves[0].is_empty());
        assert_eq!(snapshot.units.len(), 2);
        assert_eq!(game.step(Action::EndTurn).unwrap(), MatchState::Won(1));
    }

    #[test]
    fn reentrant_kill_on_the_event_hex_stops_at_the_native_recursion_limit() {
        let bundle = scenario(r#"[event]
name=start
[kill]
id=Victim
fire_event=yes
[/kill]
[/event]
[event]
name=die
first_time_only=no
[filter_second]
id=Victim
[/filter_second]
[gold]
side=2
amount=1
[/gold]
[kill]
x,y=$x1,$y1
fire_event=yes
[/kill]
[/event]"#);
        let game = bundle.start(4, |_| unreachable!()).unwrap();
        let snapshot = game.audit_snapshot();
        assert_eq!(snapshot.gold[1], 109);
        assert_eq!(snapshot.units.len(), 2);
        assert_eq!(snapshot.reserves[0].len(), 1);
    }
}

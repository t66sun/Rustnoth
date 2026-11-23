//! Select the attack hex facing the tile the cursor came from, as in mouse_handler.
use engine_lite::{
    fixed_match::{Action, FixedMatch, attack_preview::AttackPreview},
    hex::{Direction, Hex},
    observation::Observation,
};

pub struct AttackRoute {
    pub origin: Hex,
    pub staging: Hex,
    pub target: Hex,
    pub weapons: Vec<AttackPreview>,
}

#[derive(Default)]
pub(crate) struct Cursor {
    last: Option<(Hex, Hex, bool)>,
    free: Option<Hex>,
    pub(crate) attack: Option<AttackRoute>,
}

impl Cursor {
    pub(crate) fn update(&mut self, endpoints: Option<(Hex, Hex)>, allow_move_attack: bool,
        view: Option<&Observation>, actions: &[Action], game: &FixedMatch<'_>) {
        let Some(((origin, target), view)) = endpoints.zip(view) else {
            *self = Self::default();
            return;
        };
        let key = (origin, target, allow_move_attack);
        if self.last == Some(key) { return; }
        let previous = self.last.filter(|(source, _, _)| *source == origin).map(|(_, at, _)| at);
        if previous.is_none() { self.free = None; }
        self.last = Some(key);
        let direct = actions.iter().any(|action|
            matches!(action, Action::Attack(choice) if choice.from == origin && choice.to == target));
        let unit_at = |at| view.units.iter().find(|unit| unit.position == at);
        self.attack = None;
        if target == origin || unit_at(target).is_none() {
            self.free = Some(target);
            return;
        }
        if !unit_at(origin).is_some_and(|unit| unit.side == view.side && unit.attacks_left.unwrap_or(0) > 0) ||
            !unit_at(target).is_some_and(|unit| !view.allied_sides.contains(&unit.side) && !unit.petrified) { return; }

        let staging = if direct && origin.distance(target) > 1 {
            // Preserve a custom weapon that can already reach its target from here.
            Some(origin)
        } else {
            let previous = previous.unwrap_or(origin);
            attack_hex(target, previous, self.free.unwrap_or(origin), |at| {
                if at == origin { return direct; }
                allow_move_attack && unit_at(at).is_none() && actions.contains(&Action::Move { from: origin, to: at })
            })
        };
        if let Some(staging) = staging {
            let mut weapons = game.attack_previews_from(origin, staging, target);
            if staging == origin { weapons.retain(|row| actions.contains(&Action::Attack(row.choice))); }
            if !weapons.is_empty() {
                self.attack = Some(AttackRoute { origin, staging, target, weapons });
            }
        }
    }
}

fn attack_hex(target: Hex, previous: Hex, free: Hex, reachable: impl Fn(Hex) -> bool) -> Option<Hex> {
    let direction = |at| Direction::CLOCKWISE.into_iter().position(|dir| Some(dir) == target.direction_to(at));
    let preferred = direction(previous);
    let second = direction(free);
    let difference = |index: usize, other: Option<usize>| other.map_or(0, |other| {
        let distance = index.abs_diff(other);
        distance.min(6 - distance)
    });
    target.neighbors().enumerate().filter(|(_, at)| reachable(*at))
        .min_by_key(|(index, _)| {
            let first = difference(*index, preferred);
            2 * first + usize::from(difference(*index, second) > first)
        }).map(|(_, at)| at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hovering_enemy_previews_the_approach_without_moving_and_confirms_the_same_weapon() {
        use crate::session::Session;
        use engine_lite::scenario_bundle::Bundle;
        let bundle = Bundle::parse(br#"
[terrain_type]
id=flat
string=Gg
[/terrain_type]
[units]
[movetype]
name=foot
[movement_costs]
flat=1
[/movement_costs]
[/movetype]
[unit_type]
id=Fighter
movement_type=foot
movement=4
hitpoints=32
[attack]
range=melee
damage=5
number=1
[/attack]
[attack]
range=ranged
damage=3
number=2
[/attack]
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Fighter
[/side]
[side]
type=Fighter
[/side]
[/scenario]"#).unwrap();
        let mut session = Session::new(bundle.start(3, |_| unreachable!()).unwrap());
        let origin = Hex { x: 0, y: 0 };
        let staging = Hex { x: 2, y: 0 };
        let target = Hex { x: 3, y: 0 };
        session.track_pointer(Some((origin, target)), false);
        assert!(session.hovered_attack().is_none(), "scripted step allows movement only");
        session.track_pointer(Some((origin, target)), true);
        let plan = session.hovered_attack().unwrap();
        assert_eq!((plan.origin, plan.staging, plan.target), (origin, staging, target));
        let ranged = plan.weapons[1].clone();
        assert_eq!(ranged.choice.weapon, 1);
        assert_eq!(session.hovered_move().unwrap().steps.last().unwrap().0, staging);
        session.track_pointer(None, true);
        assert!(session.hovered_attack().is_none() && session.hovered_move().is_none());
        assert!(session.history().is_empty());
        assert!(!session.can_finish_attack(&ranged));
        let mut walk = Vec::new();
        session.apply_recording_walk(Action::Move { from: origin, to: staging }, None, Some(&mut walk)).unwrap();
        assert_eq!(walk.last(), Some(&staging));
        assert!(session.can_finish_attack(&ranged));
        let current = &session.attack_previews(staging, target)[1];
        assert_eq!((current.offense.damage, current.offense.hit_chance), (ranged.offense.damage, ranged.offense.hit_chance));
        session.apply(Action::Attack(ranged.choice)).unwrap();
        assert_eq!(session.history().len(), 2);
    }

    #[test]
    fn entering_from_another_flank_changes_the_attack_hex_and_skips_blocked_neighbors() {
        let target = Hex { x: 3, y: 3 };
        let north = target.neighbor(Direction::North).unwrap();
        let south = target.neighbor(Direction::South).unwrap();
        let southeast = target.neighbor(Direction::SouthEast).unwrap();
        assert_eq!(attack_hex(target, north, north, |_| true), Some(north));
        assert_eq!(attack_hex(target, south, north, |_| true), Some(south));
        assert_eq!(attack_hex(target, south, southeast, |at| at != south), Some(southeast));
        assert_eq!(attack_hex(target, south, south, |_| false), None);
    }
}

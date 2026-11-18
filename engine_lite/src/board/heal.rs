//! Mechanics of the 1.18.8 [heal_unit] WML action.
use super::Battlefield;
use crate::{
    hex::Hex,
    scenario_events::{ArrivingUnit, UnitFilter},
    wml::Node,
};

#[derive(Clone, Debug)]
enum Amount {
    Full,
    Add(i32),
}

impl Amount {
    fn read(value: &str) -> Option<Self> {
        match value.trim() {
            "full" => Some(Self::Full),
            number => number.parse().ok().map(Self::Add),
        }
    }

    fn apply(&self, current: i32, maximum: i32, minimum: i32) -> i32 {
        match self {
            Self::Full => maximum,
            Self::Add(amount) => current.saturating_add(*amount).min(maximum).max(minimum),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HealUnit {
    filter: Option<UnitFilter>,
    hp: Amount,
    moves: Amount,
    restore_attacks: bool,
    restore_statuses: bool,
}

impl HealUnit {
    pub(crate) fn read(cfg: &Node) -> Option<Self> {
        if cfg.attributes.iter().any(|(key, value)| {
            !matches!(key.as_str(), "amount" | "moves" | "restore_attacks" | "restore_statuses" | "animate")
                || !value.translations.is_empty() || value.text.contains('$')
        }) || cfg.children.iter().any(|(tag, _)| !matches!(tag.as_str(), "filter" | "filter_second")) {
            return None;
        }
        let filter = match cfg.children.iter().find(|(tag, _)| tag == "filter") {
            Some((_, cfg)) => Some(UnitFilter::read(cfg)?),
            None => None,
        };
        // In the original action filter_second selects the healer to animate only.
        let flag = |key, default| match cfg.attribute(key) {
            None => Some(default),
            Some("yes" | "true") => Some(true),
            Some("no" | "false") => Some(false),
            _ => None,
        };
        Some(Self {
            filter,
            hp: Amount::read(cfg.attribute("amount").unwrap_or("full"))?,
            moves: Amount::read(cfg.attribute("moves").unwrap_or("0"))?,
            restore_attacks: flag("restore_attacks", false)?,
            restore_statuses: flag("restore_statuses", true)?,
        })
    }

    pub(crate) fn apply(&self, board: &mut Battlefield<'_>, primary: Option<Hex>) {
        for (&position, unit) in &mut board.units {
            let matches = match &self.filter {
                Some(filter) => filter.matches(&ArrivingUnit {
                    position, side: unit.side, id: unit.id.as_deref(), type_id: &unit.unit_type.id,
                }),
                None => Some(position) == primary,
            };
            if !matches { continue; }
            unit.hitpoints = self.hp.apply(unit.hitpoints, unit.max_hitpoints(), 1);
            unit.moves_left = self.moves.apply(unit.movement_left(), unit.max_movement(), 0);
            if self.restore_attacks {
                unit.attacks_left = unit.unit_type.max_attacks;
            }
            if self.restore_statuses {
                unit.poisoned = false;
                unit.petrified = false;
                unit.slowed = false;
                unit.unhealable = false;
                if let Some(mods) = &mut unit.traits { mods.stats.unhealable = false; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{fixed_match::Action, hex::Hex, scenario_bundle::Bundle};

    #[test]
    fn filtered_healing_restores_the_named_patient_and_respects_explicit_options() {
        let bundle = Bundle::parse(br#"
[units]
[unit_type]
id=Guard
hitpoints=30
movement=6
attacks=2
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[unit]
type=Guard
id=patient
x,y=2,1
hitpoints=7
moves=1
attacks_left=0
[status]
poisoned=yes
petrified=yes
slowed=yes
unhealable=yes
[/status]
[/unit]
[unit]
type=Guard
id=other
x,y=3,1
hitpoints=9
[/unit]
[/side]
[event]
name=prestart
[heal_unit]
amount=4
restore_statuses=no
[filter]
side=2
id=patient
[/filter]
[/heal_unit]
[/event]
[event]
name=start
[heal_unit]
[filter]
role=unsupported
[/filter]
[/heal_unit]
[/event]
[event]
name=side 1 turn end
[heal_unit]
moves=full
restore_attacks=yes
[filter]
id=patient
[/filter]
[/heal_unit]
[/event]
[/scenario]"#).unwrap();
        let mut game = bundle.start(3, |_| unreachable!()).unwrap();
        let snap = game.audit_snapshot();
        let patient = snap.units.iter().find(|u| u.position == Hex { x: 1, y: 0 }).unwrap();
        assert_eq!((patient.hitpoints, patient.moves_left, patient.attacks_left), (11, 0, 0));
        assert!(patient.poisoned && patient.slowed && patient.petrified);
        // End the turn so that the second event runs via the normal action path.
        game.step(Action::EndTurn).unwrap();
        let snap = game.audit_snapshot();
        let patient = snap.units.iter().find(|u| u.position == Hex { x: 1, y: 0 }).unwrap();
        assert_eq!((patient.hitpoints, patient.moves_left, patient.attacks_left), (30, 6, 2));
        assert!(!patient.poisoned && !patient.petrified && !patient.slowed);
        let other = snap.units.iter().find(|u| u.position == Hex { x: 2, y: 0 }).unwrap();
        assert!(other.hitpoints < 30);
    }
}

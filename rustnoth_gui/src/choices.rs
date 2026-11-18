use engine_lite::{fixed_match::Action, hex::Hex, observation::Observation};

use crate::{assets::AssetCatalog, selection::Selection};

#[derive(Debug, PartialEq, Eq)]
pub struct ActionOption {
    pub label: String,
    pub action: Action,
}

/// Labels the referee's attack and recruit choices at the target hex.
pub fn options_at(
    selection: &Selection,
    target: Hex,
    view: &Observation,
    actions: &[Action],
    assets: &AssetCatalog,
) -> Vec<ActionOption> {
    let attacker_type = selection.unit().and_then(|position| {
        view.units
            .iter()
            .find(|unit| unit.position == position && unit.side == view.side)
            .map(|unit| unit.type_id.as_str())
    });

    selection
        .actions_at(target, actions)
        .filter_map(|action| {
            let label = match action {
                Action::Attack(attack) => attacker_type
                    .and_then(|id| assets.weapon_name(id, attack.weapon))
                    .map_or_else(|| format!("Weapon {}", attack.weapon + 1), str::to_owned),
                Action::Recruit { unit_type, .. } => {
                    assets.unit_name(unit_type).unwrap_or(unit_type).to_owned()
                }
                Action::Move { .. } | Action::EndTurn => return None,
            };
            Some(ActionOption { label, action: action.clone() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use engine_lite::{board::AttackChoice, observation::ObservedUnit};

    use super::*;

    #[test]
    fn labels_tactical_options_in_referee_order() {
        let core = br#"
[units]
    [unit_type]
        id=Bowman
        name= _ "Bowman"
        [attack]
            name="short sword"
        [/attack]
        [attack]
            description= _ "bow"
        [/attack]
    [/unit_type]
    [unit_type]
        id=Scout
        name= _ "Dune Scout"
    [/unit_type]
[/units]
"#;
        let assets = AssetCatalog::parse(Path::new("wesnoth_src"), core, b"").unwrap();
        let own = Hex { x: 0, y: 0 };
        let enemy = Hex { x: 1, y: 0 };
        let keep = Hex { x: 0, y: 1 };
        let view = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: Some(25),
            width: 2,
            height: 2,
            tiles: Vec::new(),
            units: vec![ObservedUnit {
                position: own,
                side: 1,
                type_id: "Bowman".to_owned(),
                hitpoints: 33,
                moves_left: Some(5),
                attacks_left: Some(1),
            }],
        };
        let bow = Action::Attack(AttackChoice { from: own, to: enemy, weapon: 1 });
        let sword = Action::Attack(AttackChoice { from: own, to: enemy, weapon: 0 });
        let scout = Action::Recruit { unit_type: "Scout".to_owned(), to: keep };
        let bowman = Action::Recruit { unit_type: "Bowman".to_owned(), to: keep };
        let actions = [
            bow.clone(),
            scout.clone(),
            Action::Move { from: own, to: keep },
            sword.clone(),
            bowman.clone(),
            Action::EndTurn,
        ];
        let mut selection = Selection::default();

        assert!(selection.select_own(own, &view));
        assert_eq!(
            options_at(&selection, enemy, &view, &actions, &assets),
            [
                ActionOption { label: "bow".to_owned(), action: bow },
                ActionOption { label: "short sword".to_owned(), action: sword },
            ],
        );
        assert!(options_at(&selection, keep, &view, &actions, &assets).is_empty());

        selection.clear();
        assert_eq!(
            options_at(&selection, keep, &view, &actions, &assets),
            [
                ActionOption { label: "Dune Scout".to_owned(), action: scout },
                ActionOption { label: "Bowman".to_owned(), action: bowman },
            ],
        );
    }
}

use engine_lite::{fixed_match::{Action, RecallInfo}, hex::Hex, observation::Observation};

use crate::{assets::AssetCatalog, selection::Selection};

#[derive(Debug, PartialEq, Eq)]
pub struct ActionOption {
    pub label: String,
    pub action: Action,
}

/// Labels the active side's legal choices at the target hex.
pub fn options_at<'a>(
    selection: &Selection,
    target: Hex,
    view: &Observation,
    actions: &[Action],
    assets: &AssetCatalog,
    recall_info: impl Fn(&str) -> Option<RecallInfo<'a>>,
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
                Action::Recall { unit_id, .. } => {
                    let unit = recall_info(unit_id)?;
                    let name = assets.unit_name(unit.type_id).unwrap_or(unit.type_id);
                    format!("Recall {name}\n{unit_id}\nHP {}   XP {}   {} gold", unit.hitpoints, unit.experience, unit.cost)
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
    fn castle_menu_distinguishes_veterans_and_switches_to_the_incoming_roster() {
        use crate::session::Session;
        use engine_lite::scenario_bundle::Bundle;
        let config = br#"
[terrain_type]
string=Kh
recruit_from=yes
recruit_onto=yes
[/terrain_type]
[terrain_type]
string=Ch
recruit_onto=yes
[/terrain_type]
[units]
    [unit_type]
        id=Guard
        name= _ "Palace Guard"
        hitpoints=40
    [/unit_type]
[/units]
[scenario]
    turns=1
    map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch,2 Kh,Xv
Xv,Xv,Xv,Xv,Xv"
    [side]
        type=Guard
        recall_cost=18
        [unit]
            id=first
            placement=recall
            type=Guard
            hitpoints=19
            experience=11
            recall_cost=13
        [/unit]
        [unit]
            id=second
            placement=recall
            type=Guard
            experience=23
        [/unit]
    [/side]
    [side]
        type=Guard
        [unit]
            id=opponent
            type=Guard
            placement=recall
        [/unit]
    [/side]
[/scenario]"#;
        let bundle = Bundle::parse(config).unwrap();
        let mut session = Session::new(bundle.start(8, |_| unreachable!()).unwrap());
        let assets = AssetCatalog::parse(Path::new("wesnoth_src"), config, b"").unwrap();
        let castle = Hex { x: 1, y: 0 };
        let menu = options_at(&Selection::default(), castle, session.observation().unwrap(),
            session.legal_actions(), &assets, |id| session.recall_info(id));
        assert_eq!(menu.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(), [
            "Recall Palace Guard\nfirst\nHP 19   XP 11   13 gold",
            "Recall Palace Guard\nsecond\nHP 40   XP 23   18 gold",
        ]);
        assert!(session.recall_info("opponent").is_none());
        session.apply(menu[0].action.clone()).unwrap();
        assert!(session.recall_info("first").is_none());
        session.apply(Action::EndTurn).unwrap();
        assert!(session.recall_info("second").is_none());
        assert!(session.recall_info("opponent").is_some());
        session.apply(Action::EndTurn).unwrap();
        assert!(session.recall_info("opponent").is_none());
    }

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
                poisoned: false,
                slowed: false,
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
            options_at(&selection, enemy, &view, &actions, &assets, |_| None),
            [
                ActionOption { label: "bow".to_owned(), action: bow },
                ActionOption { label: "short sword".to_owned(), action: sword },
            ],
        );
        assert!(options_at(&selection, keep, &view, &actions, &assets, |_| None).is_empty());

        selection.clear();
        assert_eq!(
            options_at(&selection, keep, &view, &actions, &assets, |_| None),
            [
                ActionOption { label: "Dune Scout".to_owned(), action: scout },
                ActionOption { label: "Bowman".to_owned(), action: bowman },
            ],
        );
    }
}

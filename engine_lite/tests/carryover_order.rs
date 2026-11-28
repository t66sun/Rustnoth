use engine_lite::{fixed_match::{Action, carryover::CampaignCarryover}, scenario_bundle::Bundle};

#[test]
fn carryover_writes_deployed_survivors_in_creation_order_then_existing_reserves() {
    let source = br#"[units]
[unit_type]
id=Guard
hitpoints=30
[/unit_type]
[/units]
[scenario]
next_scenario=next
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,Gg,Gg,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
id=Deoran
controller=human
type=Guard
[unit]
id=Moreth
type=Guard
x,y=2,1
[/unit]
[unit]
id=reserve
type=Guard
x,y=recall,recall
[/unit]
[unit]
id=Gerrick
type=Guard
x,y=1,1
[/unit]
[/side]
[side]
id=foe
type=Guard
[/side]
[event]
name=side 1 turn end
[endlevel]
result=victory
[/endlevel]
[/event]
[/scenario]"#;
    let bundle = Bundle::parse(source.as_slice()).unwrap();
    let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
    game.step(Action::EndTurn).unwrap();
    let mut route = CampaignCarryover::default();
    game.store_campaign_survivors(&mut route);
    let config = route.replay_roster();
    let side = &config.children.iter().find(|(tag, side)| tag == "side" && side.attribute("save_id") == Some("Deoran")).unwrap().1;
    let ids: Vec<_> = side.children.iter().filter(|(tag, _)| tag == "unit")
        .map(|(_, unit)| unit.attribute("id").unwrap()).collect();
    assert_eq!(ids, ["Deoran", "Moreth", "Gerrick", "reserve"]);
}

use super::*;
use engine_lite::{scenario_bundle::Bundle, wml};

pub(in crate::tutorial) fn turn_four() -> String {
    let feedback = |weapon, trait_id, filter| format!(
        "[event]\nname=attack_end\n{filter}\n[filter_attack]\nname={weapon}\n[/filter_attack]\n\
         [if]\n[have_unit]\nid=$unit.id\ntrait={trait_id}\n[/have_unit]\n\
         [then]\n[message]\nspeaker=narrator\nmessage={weapon} with {trait_id}\n[/message]\n[/then]\n\
         [else]\n[message]\nspeaker=narrator\nmessage=regular {weapon}\n[/message]\n[/else]\n[/if]\n[/event]");
    let sword = feedback("sword", "strong", "[filter]\ntype=Elvish Fighter\n[/filter]");
    let bow = feedback("bow", "dextrous", "");
    format!("[event]\nname=turn 4\n[filter_condition]\n[have_unit]\nid=Quintain\n[/have_unit]\n[/filter_condition]\n\
        [message]\nspeaker=narrator\nmessage=Send your fighters, Konrad\nfemale_message=female^Send your fighters, Lisar\n[/message]\n\
        [hint_message]\ncaption=Traits\nmessage=Examine the recruits\n[/hint_message]\n[print]\ntext=Attack the quintain\n[/print]\n\
        [event]\nname=select\n[filter]\nid=student\n[/filter]\n[message]\nspeaker=narrator\nmessage=Use the fighters first\n[/message]\n[/event]\n\
        {sword}\n{bow}\n[/event]")
}

#[test]
fn the_attacking_elfs_weapon_and_trait_choose_a_once_only_explanation() {
    for (trait_id, weapon_index, expected) in [
        ("strong", 0, "sword with strong"), ("strong", 1, "regular bow"),
        ("dextrous", 0, "regular sword"), ("dextrous", 1, "bow with dextrous"),
    ] {
        let source = format!("[units]\n[unit_type]\nid=Elvish Fighter\nhitpoints=30\n\
            [attack]\nname=sword\nrange=melee\ndamage=1\nnumber=1\n[/attack]\n\
            [attack]\nname=bow\nrange=ranged\ndamage=1\nnumber=1\n[/attack]\n[/unit_type]\n\
            [unit_type]\nid=Quintain\nhitpoints=40\n[/unit_type]\n[/units]\n\
            [scenario]\nmap_data=\"Xv,Xv,Xv,Xv,Xv\nXv,1 Kh,Gg,2 Kh,Xv\nXv,Xv,Xv,Xv,Xv\"\n\
            [side]\nid=student\ntype=Elvish Fighter\n[unit]\nx,y=2,1\ntype=Elvish Fighter\nrandom_traits=no\n\
            [modifications]\n[trait]\nid={trait_id}\nname=translated name\n[/trait]\n[/modifications]\n[/unit]\n[/side]\n\
            [side]\nid=Quintain\ntype=Quintain\n[/side]\n[/scenario]");
        let bundle = Bundle::parse(source.as_bytes()).unwrap();
        let mut session = Session::new(bundle.start(3, |_| unreachable!()).unwrap());
        let scenario = wml::parse(turn_four()).unwrap();
        let mut exercise = Exercise::read(&scenario, Hero::Konrad).unwrap();
        assert!(exercise.start(&mut session));
        while session.dialogue().is_some() { session.dismiss_dialogue(); }
        let from = Hex::from_wml(2, 1).unwrap();
        let attack = session.legal_actions().iter().find(|a| matches!(a,
            Action::Attack(a) if a.from == from && a.weapon == weapon_index)).unwrap().clone();
        let hint = exercise.weapon(&session, &attack).unwrap();
        session.apply(attack.clone()).unwrap();
        exercise.explain(hint, from, &mut session);
        assert_eq!(session.dialogue().unwrap().text, expected);
        assert!(exercise.weapon(&session, &attack).is_none());
        assert!(!exercise.prompting);

        let absent = source.replace("id=Quintain\ntype=Quintain", "id=another_dummy\ntype=Quintain");
        let bundle = Bundle::parse(absent.as_bytes()).unwrap();
        let mut session = Session::new(bundle.start(3, |_| unreachable!()).unwrap());
        let mut exercise = Exercise::read(&scenario, Hero::Lisar).unwrap();
        assert!(!exercise.start(&mut session));
        assert!(session.dialogue().is_none());
    }
}

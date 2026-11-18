use crate::wml::Node;

/// All authored human sides are local in the Rust campaign player. The stock
/// endlevel.lua reports victory when any local human wins. An AI-only scenario
/// uses the command's overall result.
pub(super) fn player_victory(cfg: &Node, scenario: &Node) -> Option<bool> {
    fn result(cfg: &Node, fallback: bool) -> Option<bool> {
        match cfg.attributes.get("result") {
            None => Some(fallback),
            Some(v) if v.translations.is_empty() => match v.text.as_str() {
                "victory" => Some(true),
                "defeat" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    let default = result(cfg, true)?;
    let mut sides: Vec<_> = scenario.children.iter().filter(|(tag, _)| tag == "side")
        .map(|(_, side)| (side.attribute("controller") == Some("human"), default)).collect();
    for (tag, entry) in &cfg.children {
        if tag != "result" || !entry.children.is_empty() { return None; }
        let number = entry.attribute("side")?.trim().parse::<usize>().ok()?;
        let victory = result(entry, default)?;
        if let Some((_, selected)) = number.checked_sub(1).and_then(|i| sides.get_mut(i)) {
            *selected = victory;
        }
    }
    let mut players = sides.iter().filter_map(|&(human, victory)| human.then_some(victory));
    Some(players.next().map_or(default, |first| first || players.any(|won| won)))
}

#[cfg(test)]
mod tests {
    use crate::{fixed_match::{Action, MatchError, MatchState}, scenario_bundle::Bundle};

    fn bundle(event: &str, fields: &str) -> Bundle {
        let text = format!(r#"[units]
[unit_type]
id=Guard
hitpoints=22
[/unit_type]
[/units]
[scenario]
turns=1
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
controller=ai
type=Guard
[/side]
[side]
controller=human
type=Guard
[/side]
[event]
name={event}
[message]
speaker=narrator
message=The battle is over.
[/message]
[endlevel]
{fields}
[/endlevel]
[/event]
[/scenario]"#);
        Bundle::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn opening_and_turn_events_end_the_match_for_the_authored_player() {
        for (event, steps) in [("start", 0), ("side 1 turn end", 1), ("side 2 turn", 1),
                               ("side 2 turn refresh", 1), ("time over", 2)] {
            for (fields, expected) in [
                ("", MatchState::CampaignVictory),
                ("result=victory\n[result]\nside=2\nresult=defeat\n[/result]", MatchState::CampaignDefeat),
            ] {
                let scenario = bundle(event, fields);
                let mut game = scenario.start(1, |_| unreachable!()).unwrap();
                for _ in 0..steps { game.step(Action::EndTurn).unwrap(); }
                assert_eq!(game.state(), expected, "{event}");
                assert!(game.decision().is_none());
                assert_eq!(game.step(Action::EndTurn), Err(MatchError::Finished));
            }
        }
    }
}

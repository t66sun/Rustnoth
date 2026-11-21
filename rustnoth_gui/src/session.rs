use engine_lite::{
    fixed_match::{Action, FixedMatch, MatchError, MatchState, RecallInfo},
    hex::Hex,
    observation::Observation,
    opponent::attack_or_approach,
    scenario_sides::SideController,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Players {
    HotSeat,
    VersusComputer,
    Campaign,
}

impl Players {
    fn controller(self, game: &FixedMatch<'_>, side: usize) -> SideController {
        match self {
            Self::HotSeat => SideController::Human,
            Self::VersusComputer if side == 1 => SideController::Human,
            Self::VersusComputer => SideController::Ai,
            Self::Campaign => game.side_controllers().nth(side - 1).expect("active match side"),
        }
    }

    fn viewing_side(self, game: &FixedMatch<'_>, side: usize) -> Option<usize> {
        match self {
            Self::HotSeat => Some(side),
            Self::VersusComputer => Some(1),
            Self::Campaign => {
                // play_controller::find_viewing_side starts at the active side and wraps.
                let roster = game.side_controllers().enumerate();
                roster.clone().skip(side - 1).chain(roster.take(side - 1))
                    .find(|(_, controller)| *controller == SideController::Human)
                    .map(|(index, _)| index + 1)
            }
        }
    }
}

/// Current human view and the next command for a computer-controlled side.
pub struct Session<'rules> {
    game: FixedMatch<'rules>,
    animate_combat: bool,
    live_attack_seed: Option<fn() -> u32>,
    last_combat: Option<engine_lite::fixed_match::combat_display::VisibleCombat>,
    dialogue: std::collections::VecDeque<engine_lite::fixed_match::dialogue::DialogueLine>,
    players: Players,
    history: Vec<crate::save_replay::Event>,
    economy: Option<engine_lite::income::EconomyReport>,
    hovered_move: Option<(Hex, Hex)>,
    route: Option<engine_lite::fixed_match::move_preview::MovePreview>,
    computer_action: Option<Action>,
    observation: Option<Observation>,
    legal_actions: Vec<Action>,
}

impl<'rules> Session<'rules> {
    pub fn new(game: FixedMatch<'rules>) -> Self {
        Self::with_players(game, Players::HotSeat)
    }

    pub fn with_players(game: FixedMatch<'rules>, players: Players) -> Self {
        let mut session = Self {
            economy: None,
            animate_combat: false, live_attack_seed: None, last_combat: None,
            hovered_move: None, route: None,
            dialogue: Default::default(), game, players, history: Vec::new(), observation: None, legal_actions: Vec::new(), computer_action: None,
        };
        session.refresh();
        session
    }

    fn refresh(&mut self) {
        self.hovered_move = None;
        self.route = None;
        self.computer_action = None;
        self.legal_actions.clear();
        match self.game.decision() {
            Some((view, actions)) => {
                match self.players.controller(&self.game, view.side) {
                    SideController::Human => {
                        self.observation = Some(view);
                        self.legal_actions = actions;
                    }
                    controller => {
                        self.computer_action = Some(if controller == SideController::Ai {
                            computer_choice(&view, &actions)
                        } else {
                            Action::EndTurn
                        });
                        self.observation = self.players.viewing_side(&self.game, view.side)
                            .and_then(|viewer| self.game.observation_for(viewer));
                    }
                }
            }
            None => {
                let last_side = self.observation.as_ref().map_or(1, |view| view.side);
                self.observation = self.players.viewing_side(&self.game, last_side)
                    .and_then(|viewer| self.game.observation_for(viewer));
            }
        }
        self.economy = self.observation.as_ref().and_then(|view| self.game.economy_for(view));
        if let Some(viewer) = self.observation.as_ref().map(|view| view.side) {
            self.dialogue.extend(self.game.take_dialogue(viewer));
        }
    }

    pub fn dialogue(&self) -> Option<&engine_lite::fixed_match::dialogue::DialogueLine> {
        self.dialogue.front()
    }

    pub fn economy(&self) -> Option<engine_lite::income::EconomyReport> { self.economy }

    pub fn history(&self) -> &[crate::save_replay::Event] { &self.history }

    pub fn has_unused_unit(&self, unmoved_only: bool) -> bool {
        self.observation().is_some_and(|view| self.game.has_unused_unit(view, unmoved_only))
    }

    pub fn dismiss_dialogue(&mut self) {
        if self.dialogue.pop_front().is_some() { self.history.push(crate::save_replay::Event::Dismiss); }
    }

    /// Enabled by the live board after replaying any saved commands.
    pub fn enable_combat_playback(&mut self) {
        self.animate_combat = true;
    }

    pub fn take_combat(&mut self) -> Option<engine_lite::fixed_match::combat_display::VisibleCombat> {
        self.last_combat.take()
    }

    pub fn enable_fresh_attack_seeds(&mut self) {
        // Each RandomState uses fresh hash keys and requires no GUI/global PRNG state.
        self.live_attack_seed = Some(|| {
            use std::hash::{BuildHasher, Hasher, RandomState};
            RandomState::new().build_hasher().finish() as u32
        });
    }

    fn live_seed(&self, action: Option<&Action>) -> Option<u32> {
        match (action, self.live_attack_seed) {
            (Some(Action::Attack(_)), Some(next)) => Some(next()),
            _ => None,
        }
    }

    fn commit(&mut self, action: Action, advancement: Option<&str>, seed: Option<u32>) -> Result<MatchState, MatchError> {
        self.last_combat = None;
        let Action::Attack(choice) = action else {
            if seed.is_some() { return Err(MatchError::ActionUnavailable); }
            return self.game.step(action);
        };
        let viewer = self.observation.as_ref().filter(|_| self.animate_combat).map(|view| view.side);
        let (state, fight) = self.game.attack_with_action_seed(choice, advancement, viewer, seed)?;
        self.last_combat = fight;
        Ok(state)
    }

    /// Apply at most one computer command per desktop frame.
    pub fn play_computer_action(&mut self) -> Result<bool, MatchError> {
        self.play_computer_recording_walk(None)
    }

    pub fn play_computer_recording_walk(&mut self, display: Option<&mut Option<engine_lite::fixed_match::walk::VisibleWalk>>)
        -> Result<bool, MatchError> {
        let seed = self.live_seed(self.computer_action.as_ref());
        self.play_computer_display(seed, display)
    }

    pub(crate) fn play_computer_with_seed(&mut self, seed: Option<u32>) -> Result<bool, MatchError> {
        self.play_computer_display(seed, None)
    }

    fn play_computer_display(&mut self, seed: Option<u32>,
        display: Option<&mut Option<engine_lite::fixed_match::walk::VisibleWalk>>) -> Result<bool, MatchError> {
        if self.dialogue().is_some() { return Ok(false); }
        let Some(action) = self.computer_action.take() else { return Ok(false) };
        let result = match (&action, display, &self.observation) {
            (Action::Move { from, to }, Some(walk), Some(view)) if seed.is_none() => self.game.move_for_viewer(*from, *to, view.side, walk),
            _ => self.commit(action.clone(), None, seed),
        };
        match result {
            Ok(_) => {}
            // A castle hex offered through fog can turn out to be occupied.
            Err(MatchError::InvalidRecruit) if matches!(action, Action::Recruit { .. }) => {
                self.game.step(Action::EndTurn)?;
            }
            Err(error) => return Err(error),
        }
        self.refresh();
        // A hidden occupant can block a teleport without revealing its location.
        if matches!(action, Action::Move { .. }) && self.computer_action.as_ref() == Some(&action) {
            self.computer_action = Some(Action::EndTurn);
        }
        self.history.push(crate::save_replay::Event::Computer(seed));
        Ok(true)
    }

    pub fn store_campaign_survivors(&self, route: &mut engine_lite::fixed_match::carryover::CampaignCarryover) {
        self.game.store_campaign_survivors(route);
    }

    pub fn store_campaign_gold(&self, gold: &mut engine_lite::fixed_match::carryover::CampaignGold) {
        self.game.store_campaign_gold(gold);
    }

    pub fn show_end_report(&self) -> bool { self.game.show_end_report() }
    pub fn save_victory_replay(&self) -> bool { self.game.save_victory_replay() }

    pub fn retained_gold(&self) -> Option<i64> {
        self.game.retained_gold(self.observation.as_ref()?.side)
    }

    pub fn next_scenario(&self) -> Option<&str> {
        self.game.next_scenario_after_victory()
    }

    pub fn state(&self) -> MatchState {
        self.game.state()
    }

    pub fn global_time_of_day(&self) -> Option<&engine_lite::time_of_day::TimeOfDay> {
        self.game.global_time_of_day()
    }

    pub fn observation(&self) -> Option<&Observation> {
        self.observation.as_ref()
    }

    pub fn objectives(&self) -> Option<&engine_lite::scenario_sides::ScenarioObjectives> {
        self.game.objectives_for(self.observation.as_ref()?.side)
    }

    pub fn has_objective_notice(&self) -> bool {
        self.dialogue.is_empty() && !self.legal_actions.is_empty()
            && self.observation.as_ref().is_some_and(|view| self.game.objectives_changed(view.side))
    }

    pub fn has_authored_objectives(&self) -> bool {
        self.observation.as_ref().is_some_and(|view| self.game.has_authored_objectives(view.side))
    }

    pub fn evaluate_objectives(&self) -> Option<engine_lite::scenario_sides::ScenarioObjectives> {
        self.game.evaluate_objectives_for(self.observation.as_ref()?.side)
    }

    pub fn acknowledge_objectives(&mut self) -> bool {
        if !self.has_objective_notice() { return false; }
        let side = self.observation.as_ref().expect("pending objectives have a viewer").side;
        self.game.acknowledge_objectives(side);
        self.history.push(crate::save_replay::Event::ObjectivesSeen);
        true
    }

    pub fn map_item_images(&self) -> Vec<(Hex, &str)> {
        self.observation.as_ref().map(|view| self.game.map_item_images(view.side)).unwrap_or_default()
    }

    pub fn legal_actions(&self) -> &[Action] {
        &self.legal_actions
    }

    pub fn track_hovered_move(&mut self, endpoints: Option<(Hex, Hex)>) {
        if self.hovered_move == endpoints { return; }
        self.hovered_move = endpoints;
        self.route = endpoints.and_then(|(from, to)| {
            if !self.legal_actions.contains(&Action::Move { from, to }) { return None; }
            self.game.move_preview(from, to)
        });
    }

    pub fn hovered_move(&self) -> Option<&engine_lite::fixed_match::move_preview::MovePreview> {
        self.route.as_ref()
    }

    pub fn unit_report(&self, position: Hex, pointer: Hex) -> Option<engine_lite::fixed_match::unit_report::UnitReport> {
        self.game.unit_report(self.observation.as_ref()?.side, position, pointer)
    }

    pub fn attack_previews(&self, from: Hex, to: Hex) -> Vec<engine_lite::fixed_match::attack_preview::AttackPreview> {
        if self.computer_action.is_some() { return Vec::new(); }
        self.game.attack_previews(from, to)    }

    /// Limit the actions offered by the interface during a scripted tutorial step.
    pub fn retain_actions(&mut self, allowed: impl FnMut(&Action) -> bool) {
        self.legal_actions.retain(allowed);
    }

    pub fn advancement_choices(&self, position: Hex) -> Vec<String> {
        if self.computer_action.is_some() { return Vec::new(); }
        self.game.advancement_choices(position)
    }

    pub fn recall_info(&self, id: &str) -> Option<RecallInfo<'_>> {
        if self.computer_action.is_some() { return None; }
        self.game.recall_info(id)
    }

    pub fn apply(&mut self, action: Action) -> Result<MatchState, MatchError> {
        self.apply_with_advancement(action, None)
    }

    pub fn apply_with_advancement(&mut self, action: Action, attacker: Option<&str>) -> Result<MatchState, MatchError> {
        self.apply_recording_walk(action, attacker, None)
    }

    pub fn apply_recording_walk(&mut self, action: Action, attacker: Option<&str>, trace: Option<&mut Vec<Hex>>) -> Result<MatchState, MatchError> {
        let seed = self.live_seed(Some(&action));
        self.apply_with_seed(action, attacker, trace, seed)
    }

    pub(crate) fn apply_with_seed(&mut self, action: Action, attacker: Option<&str>, trace: Option<&mut Vec<Hex>>, seed: Option<u32>) -> Result<MatchState, MatchError> {
        if self.computer_action.is_some() || self.dialogue().is_some() {
            return Err(MatchError::ActionUnavailable);
        }
        let state = match (action.clone(), trace) {
            (Action::Move { from, to }, Some(trace)) => self.game.move_with_trace(from, to, trace)?,
            (action, _) => self.commit(action, attacker, seed)?,
        };
        self.history.push(crate::save_replay::Event::Player(action, attacker.map(str::to_owned), seed));
        self.refresh();
        Ok(state)
    }
}

fn computer_choice(view: &Observation, actions: &[Action]) -> Action {
    let tactical = attack_or_approach(view, actions);
    if let Some(attack @ Action::Attack(_)) = tactical {
        return attack.clone();
    }
    if let Some(recruit) = actions.iter().find(|a| matches!(a, Action::Recruit { .. })) {
        return recruit.clone();
    }
    if let Some(approach) = tactical {
        return approach.clone();
    }

    // Scout towards an unseen neighbor using only the player's known board.
    let visible: std::collections::HashSet<_> = view.tiles.iter()
        .filter(|tile| tile.visible).map(|tile| tile.position).collect();
    actions.iter().filter_map(|action| {
        let Action::Move { from, to } = action else { return None };
        let frontier = to.neighbors().filter(|hex| {
            (0..view.width).contains(&hex.x) && (0..view.height).contains(&hex.y) && !visible.contains(hex)
        }).count();
        (frontier > 0).then_some(((frontier, from.distance(*to)), action))
    }).max_by_key(|(score, _)| *score).map_or(Action::EndTurn, |(_, action)| action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{board::AttackChoice, hex::Hex, scenario_bundle::Bundle};

    #[test]
    fn campaign_ai_takes_its_turn_while_each_local_player_sees_only_their_own_view() {
        // The first side defaults to AI. The last side is explicitly empty.
        let bundle = Bundle::parse(br#"[units]
[unit_type]
id=Guard
hitpoints=20
movement=0
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Gg,3 Gg,Gg,4 Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
controller=human
type=Guard
team_name=allies
share_vision=none
fog=yes
gold=22
[/side]
[side]
controller=human
type=Guard
team_name=allies
share_vision=none
gold=33
[/side]
[side]
controller=null
type=Guard
[/side]
[event]
name=start
[message]
speaker=narrator
side_for=1
message=AI orders
[/message]
[message]
speaker=narrator
side_for=2
message=Human orders
[/message]
[/event]
[/scenario]"#).unwrap();
        let mut session = Session::with_players(bundle.start_with_controllers(1, |_| unreachable!(), None, true).unwrap(), Players::Campaign);
        let view = session.observation().unwrap();
        assert_eq!((view.side, view.gold), (2, Some(22)));
        assert_eq!(session.dialogue().unwrap().text, "Human orders");
        assert!(session.legal_actions().is_empty());
        assert!(!session.play_computer_action().unwrap());
        session.dismiss_dialogue();
        assert!(session.play_computer_action().unwrap());
        assert_eq!(session.observation().unwrap().side, 2);
        assert!(session.legal_actions().contains(&Action::EndTurn));
        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.observation().unwrap().side, 3);
        assert!(!session.play_computer_action().unwrap());
        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.observation().unwrap().side, 2);
        assert!(session.legal_actions().is_empty());
        assert_eq!(session.apply(Action::EndTurn), Err(MatchError::ActionUnavailable));
        assert_eq!(session.state(), MatchState::Playing(engine_lite::turn::Turn { round: 2, side: 1 }));
    }

    #[test]
    fn campaign_controller_changes_reassign_input_and_view_during_play_and_replay() {
        let bundle = Bundle::parse(br#"[units]
[unit_type]
id=Guard
hitpoints=20
movement=0
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
controller=human
type=Guard
[/side]
[side]
controller=null
type=Guard
fog=yes
[/side]
[event]
name=side 1 turn 1
[modify_side]
side=1
controller=ai
hidden=no
[/modify_side]
[modify_side]
side=2
controller=human
gold=23
[/modify_side]
[/event]
[event]
name=side 2 turn 1
[modify_side]
side=2
controller=null
[/modify_side]
[/event]
[event]
name=side 1 turn 2
[modify_side]
side=1
controller=human
[/modify_side]
[modify_side]
side=2
controller=null
[/modify_side]
[/event]
[/scenario]"#).unwrap();
        let start = || Session::with_players(bundle.start_with_controllers(17, |_| unreachable!(), None, true).unwrap(), Players::Campaign);
        let mut live = start();
        let view = live.observation().unwrap();
        assert_eq!((view.side, view.gold), (2, Some(23)));
        assert!(live.legal_actions().is_empty());
        assert!(live.play_computer_action().unwrap());
        assert_eq!(live.observation().unwrap().side, 2);
        // A currently playing side cannot be changed to null.
        assert!(live.legal_actions().contains(&Action::EndTurn));
        live.apply(Action::EndTurn).unwrap();
        assert_eq!(live.observation().unwrap().side, 1);
        assert_eq!(live.game.side_controllers().collect::<Vec<_>>(), [SideController::Human, SideController::Empty]);
        let mut replay = start();
        for event in live.history() { event.apply(&mut replay).unwrap(); }
        assert_eq!(replay.state(), live.state());
        assert_eq!(replay.observation(), live.observation());
        assert_eq!(replay.legal_actions(), live.legal_actions());
    }

    #[test]
    fn shows_only_the_human_viewers_messages_in_authored_order() {
        let bundle = Bundle::parse(br#"
[units]
[unit_type]
id=Guard
hitpoints=20
movement=0
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
id=captain
name=Captain Arin
profile=portraits/captain.webp
fog=yes
[/side]
[side]
type=Guard
id=bandit
[/side]
[event]
name=start
[message]
speaker=narrator
side_for=2
message=Second side only
[/message]
[message]
speaker=bandit
message=Hidden enemy
[/message]
[message]
speaker=narrator
image=wesnoth-icon.png
message=On arrival
[/message]
[message]
speaker=captain
message=Defend the fort
[/message]
[/event]
[event]
name=side 2 turn
[message]
speaker=narrator
side_for=2
message=Enemy orders
[/message]
[message]
speaker=narrator
side_for=1
message=Nightfall
[/message]
[/event]
[/scenario]"#).unwrap();
        let mut session = Session::with_players(bundle.start(1, |_| unreachable!()).unwrap(), Players::VersusComputer);
        let line = session.dialogue().unwrap();
        assert_eq!(line.text, "On arrival");
        assert_eq!(line.image.as_deref(), Some("wesnoth-icon.png"));
        assert_eq!(session.apply(Action::EndTurn), Err(MatchError::ActionUnavailable));
        session.dismiss_dialogue();
        let line = session.dialogue().unwrap();
        assert_eq!(line.text, "Defend the fort");
        assert_eq!(line.caption.as_deref(), Some("Captain Arin"));
        assert_eq!(line.image.as_deref(), Some("portraits/captain.webp"));
        session.dismiss_dialogue();
        assert!(session.dialogue().is_none());
        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.dialogue().unwrap().text, "Nightfall");
        assert!(!session.play_computer_action().unwrap());
        session.dismiss_dialogue();
        assert!(session.play_computer_action().unwrap());
    }

    #[test]
    fn computer_recruits_or_scouts_when_no_enemy_is_visible() {
        use engine_lite::observation::ObservedTile;

        let home = Hex { x: 0, y: 0 };
        let edge = Hex { x: 1, y: 0 };
        let view = Observation {
            side: 2, allied_sides: vec![2], gold: Some(50), width: 3, height: 1,
            tiles: [home, edge].into_iter().map(|position| ObservedTile {
                position, terrain: "Kh".into(), visible: true, village_owner: None, time_color: [0; 3],
            }).collect(),
            units: vec![],
        };
        let scout = Action::Move { from: home, to: edge };
        let recruit = Action::Recruit { unit_type: "Scout".into(), to: edge };
        assert_eq!(computer_choice(&view, &[scout.clone(), recruit.clone(), Action::EndTurn]), recruit);
        assert_eq!(computer_choice(&view, &[scout.clone(), Action::EndTurn]), scout);
        assert_eq!(computer_choice(&view, &[Action::EndTurn]), Action::EndTurn);
    }

    #[test]
    fn objectives_wait_for_dialogue_and_their_human_side_and_stay_read_on_reload() {
        use crate::save_replay::Event;
        let source = br#"[units]
[unit_type]
id=Scout
hitpoints=20
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
type=Scout
controller=human
[/side]
[side]
type=Scout
controller=ai
[/side]
[event]
name=prestart
[objectives]
side=0
silent=yes
[objective]
condition=win
description=Investigate the river fort
[/objective]
[/objectives]
[/event]
[event]
name=start
[message]
speaker=narrator
message=The South Guard has fallen.
[/message]
[/event]
[event]
name=side 2 turn
[show_objectives]
side=1
[/show_objectives]
[/event]
[event]
name=side 1 turn 3
[show_objectives]
side=1
[/show_objectives]
[objectives]
side=1
silent=yes
[objective]
condition=win
description=Guard the citadel
[/objective]
[/objectives]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(source).unwrap();
        let start = || Session::with_players(bundle.start(17, |_| unreachable!()).unwrap(), Players::VersusComputer);
        let mut live = start();
        assert!(live.dialogue().is_some());
        assert!(!live.acknowledge_objectives());
        live.dismiss_dialogue();
        assert!(live.has_objective_notice());
        assert!(live.acknowledge_objectives());
        assert!(!live.acknowledge_objectives());
        live.apply(Action::EndTurn).unwrap();
        assert!(!live.has_objective_notice());
        while live.play_computer_action().unwrap() {}
        assert!(live.has_objective_notice());
        let mut resumed = start();
        for event in live.history() { Event::decode(&event.encode()).unwrap().apply(&mut resumed).unwrap(); }
        assert!(resumed.has_objective_notice());
        assert!(live.acknowledge_objectives());
        live.history().last().unwrap().apply(&mut resumed).unwrap();
        assert!(!resumed.has_objective_notice());
        assert_eq!(live.history(), resumed.history());
        live.apply(Action::EndTurn).unwrap();
        while live.play_computer_action().unwrap() {}
        assert_eq!(live.objectives().unwrap().victory, ["Guard the citadel"]);
        assert!(!live.has_objective_notice());
    }

    #[test]
    fn desktop_objectives_follow_the_viewer_and_ordered_side_events() {
        let source = br#"[units]
[unit_type]
id=Scout
hitpoints=20
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
type=Scout
[/side]
[side]
type=Scout
[/side]
[event]
name=prestart
[objectives]
side=0
[objective]
condition=win
description= _ "Reach the keep"
[/objective]
[objective]
condition=lose
description=Loss of the scout
[/objective]
[objective]
condition=win
description=Secret goal
[show_if]
[have_unit]
id=hidden
[/have_unit]
[/show_if]
[/objective]
[/objectives]
[/event]
[event]
name=side 2 turn
[objectives]
side=2
[objective]
condition=win
description=Defend the keep
[/objective]
[/objectives]
[/event]
[/scenario]"#;
        let bundle = engine_lite::scenario_bundle::Bundle::parse(source).unwrap();
        let mut session = Session::new(bundle.start(7, |_| unreachable!()).unwrap());
        let mut versus = Session::with_players(bundle.start(7, |_| unreachable!()).unwrap(), Players::VersusComputer);
        versus.apply(Action::EndTurn).unwrap();
        assert_eq!(versus.objectives().unwrap().victory, ["Reach the keep"]);
        let first = session.objectives().unwrap();
        assert_eq!(first.victory, ["Reach the keep"]);
        assert_eq!(first.defeat, ["Loss of the scout"]);
        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.objectives().unwrap().victory, ["Defend the keep"]);
        assert!(session.objectives().unwrap().defeat.is_empty());
        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.objectives().unwrap().victory, ["Reach the keep"]);
    }

    #[test]
    fn shows_the_current_global_period_across_side_and_round_changes() {
        let bundle = Bundle::parse(br#"
            [units]
                [unit_type]
                    id=Guard
                    hitpoints=10
                [/unit_type]
            [/units]
            [scenario]
                turns=2
                current_time=1
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [time]
                    id=custom_light
                    lawful_bonus=13
                [/time]
                [time]
                    id=first_watch
                    lawful_bonus=-25
                [/time]
                [time_area]
                    x,y=1,1
                    [time]
                        id=local_cavern
                        red=-31
                        blue=16
                        lawful_bonus=-40
                    [/time]
                [/time_area]
                [side]
                    type=Guard
                [/side]
                [side]
                    type=Guard
                [/side]
            [/scenario]
        "#).unwrap();
        let mut session = Session::new(bundle.start(19, |_| unreachable!()).unwrap());
        let observed_colors: Vec<_> = session.observation().unwrap().tiles.iter()
            .map(|tile| (tile.position, tile.time_color)).collect();
        assert_eq!(observed_colors, [(Hex { x: 0, y: 0 }, [-31, 0, 16]), (Hex { x: 1, y: 0 }, [0; 3])]);
        for expected in [("first_watch", -25), ("first_watch", -25), ("custom_light", 13), ("custom_light", 13)] {
            let period = session.global_time_of_day().unwrap();
            assert_eq!((period.id.as_str(), period.lawful_bonus), expected);
            session.apply(Action::EndTurn).unwrap();
        }
        assert!(session.global_time_of_day().is_none());
    }

    #[test]
    fn saved_attacks_replay_their_own_seed_and_leave_future_commands_live() {
        use crate::save_replay::Event;
        let bundle = Bundle::parse(br#"
[terrain_type]
id=flat
string=Gg
[/terrain_type]
[units]
[movetype]
name=foot
[defense]
flat=50
[/defense]
[/movetype]
[unit_type]
id=Guard
movement_type=foot
hitpoints=40
[attack]
range=melee
damage=3
number=1
[/attack]
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
type=Guard
random_traits=no
[/side]
[side]
type=Guard
random_traits=no
[/side]
[/scenario]"#).unwrap();
        let start = || Session::with_players(bundle.start(11, |_| unreachable!()).unwrap(), Players::VersusComputer);
        let mut live = start();
        live.live_attack_seed = Some(|| 5489);
        let attack = Action::Attack(AttackChoice {
            from: Hex { x: 0, y: 0 }, to: Hex { x: 1, y: 0 }, weapon: 0,
        });
        live.apply(attack.clone()).unwrap();
        let saved = live.history()[0].clone();
        assert_eq!(saved, Event::Player(attack.clone(), None, Some(5489)));
        let mut restored = start();
        restored.live_attack_seed = Some(|| 3);
        Event::decode(&saved.encode()).unwrap().apply(&mut restored).unwrap();
        assert_eq!(restored.observation(), live.observation());
        assert_eq!(restored.history(), live.history());

        live.apply(Action::EndTurn).unwrap();
        restored.apply(Action::EndTurn).unwrap();
        assert!(live.play_computer_action().unwrap());
        assert_eq!(live.history().last(), Some(&Event::Computer(Some(5489))));
        Event::decode(&live.history().last().unwrap().encode()).unwrap().apply(&mut restored).unwrap();
        assert_eq!(restored.observation(), live.observation());

        let mut alternate = start();
        alternate.live_attack_seed = Some(|| 3);
        alternate.apply(attack).unwrap();
        let mut original = start();
        saved.apply(&mut original).unwrap();
        assert_ne!(alternate.observation(), original.observation());
    }

    #[test]
    fn follows_the_active_side_and_keeps_its_updated_board_at_the_end() {
        let bundle = Bundle::parse(
            br#"
            [units]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    [attack]
                        range=melee
                        damage=1
                        number=1
                    [/attack]
                [/unit_type]
                [unit_type]
                    id=Guard
                    hitpoints=10
                    [attack]
                        range=melee
                        damage=10
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                turns=1
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Fighter
                    gold=17
                [/side]
                [side]
                    side=2
                    type=Guard
                    gold=29
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let mut session = Session::new(bundle.start(5489, |_| unreachable!()).unwrap());
        let west = Hex { x: 0, y: 0 };
        let east = Hex { x: 1, y: 0 };
        let mut versus = Session::with_players(bundle.start(5489, |_| unreachable!()).unwrap(), Players::VersusComputer);
        assert!(!versus.play_computer_action().unwrap());
        versus.apply(Action::EndTurn).unwrap();
        assert_eq!(versus.observation().unwrap().side, 1);
        assert_eq!(versus.observation().unwrap().gold, Some(17));
        assert!(versus.legal_actions().is_empty());
        assert_eq!(versus.apply(Action::EndTurn), Err(MatchError::ActionUnavailable));
        assert!(versus.play_computer_action().unwrap());
        assert_eq!(versus.state(), MatchState::Won(2));
        let [guard] = versus.observation().unwrap().units.as_slice() else { panic!("expected only the guard") };
        assert_eq!((guard.side, guard.attacks_left), (2, None));
        assert!(!versus.play_computer_action().unwrap());

        let west_attack = Action::Attack(AttackChoice { from: west, to: east, weapon: 0 });
        let east_attack = Action::Attack(AttackChoice { from: east, to: west, weapon: 0 });

        assert_eq!(session.observation().unwrap().side, 1);
        assert_eq!(session.legal_actions(), [west_attack.clone(), Action::EndTurn]);
        assert!(session.apply(east_attack.clone()).is_err());
        assert_eq!(session.observation().unwrap().side, 1);
        assert_eq!(session.legal_actions(), [west_attack.clone(), Action::EndTurn]);

        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.observation().unwrap().side, 2);
        assert_eq!(session.legal_actions(), [east_attack, Action::EndTurn]);

        assert_eq!(session.apply(Action::EndTurn).unwrap(), MatchState::Draw);
        assert_eq!(session.state(), MatchState::Draw);
        let view = session.observation().unwrap();
        assert_eq!(view.side, 2);
        assert_eq!(view.units.len(), 2);
        assert!(session.legal_actions().is_empty());

        let mut session = Session::new(bundle.start(5489, |_| unreachable!()).unwrap());
        assert_eq!(session.observation().unwrap().units.len(), 2);
        assert_eq!(session.apply(west_attack).unwrap(), MatchState::Won(2));
        let view = session.observation().unwrap();
        assert_eq!((view.side, view.gold), (1, Some(17)));
        let [survivor] = view.units.as_slice() else {
            panic!("expected only the defending unit to survive");
        };
        assert_eq!((survivor.position, survivor.side, survivor.hitpoints), (east, 2, 9));
        assert_eq!((survivor.moves_left, survivor.attacks_left), (None, None));
        assert!(session.legal_actions().is_empty());
    }
}

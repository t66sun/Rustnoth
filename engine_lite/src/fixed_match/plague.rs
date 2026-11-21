use crate::{
    board::{Attack, Battlefield},
    turn::Turn,
    fixed_match::{FixedMatch, MatchError},
    hex::Hex,
    terrain_aliases::TerrainAliases,
    unit_type::UnitType,
};

struct Reanimation<'rules> {
    side: usize,
    base: &'rules UnitType,
    variation: String,
}

impl FixedMatch<'_> {
    /// Run death events while the casualty is still on the map, then resolve plague.
    pub(super) fn apply_scenario_attack(
        &mut self, turn: Turn, mut attack: Attack,
        display: Option<(usize, &mut Option<super::combat_display::VisibleCombat>)>,
    ) -> Result<bool, MatchError> {
        let (from, to) = (attack.from, attack.to);
        let weapon = attack.weapon;
        let sides = &self.scenario.sides;
        let (attacker_id, defender_id) = self.scenario.board.begin_attack(turn.side, &attack,
            |first, second| sides[first - 1].is_enemy(&sides[second - 1]))?;
        let retaliation = attack.retaliation.as_ref().map(|(index, _)| *index);
        if self.scenario.events.has_handler("attack") {
            self.fire_units_event("attack", from, Some(to))?;
            let board = &self.scenario.board;
            if !matches!(self.state(), crate::fixed_match::MatchState::Playing(_))
                || board.unit_at(from).is_none_or(|u| u.serial != attacker_id)
                || board.unit_at(to).is_none_or(|u| u.serial != defender_id)
            {
                self.fire_units_event("attack_end", from, Some(to))?;
                return Ok(false);
            }
            // The original refreshes both weapons' modifiers after attack handlers.
            let attacker = board.unit_at(from).unwrap();
            let defender = board.unit_at(to).unwrap();
            let refresh = |origin, unit: &crate::board::UnitState<'_>, target: &crate::board::UnitState<'_>, index: usize, dest, offense| {
                self.modifiers(self.lawful_bonus_at(turn.round, origin), unit, target, &unit.unit_type.attacks[index],
                    dest, self.leadership_bonus(origin, unit), offense)
            };
            attack.modifiers = refresh(from, attacker, defender, weapon, to, true);
            attack.retaliation = retaliation.map(|i| (i, refresh(to, defender, attacker, i, from, false)));
        }
        let display = display.and_then(|(viewer, slot)| {
            *slot = self.visible_combat(viewer, &attack);
            slot.as_mut().map(|fight| &mut fight.blows)
        });
        let sides = &self.scenario.sides;
        // Native attack_end runs before combat XP is paid to the survivors.
        let prior_experience = self.scenario.board.unit_at(from).expect("validated attacker").experience;
        let result = self.scenario.board.resolve_started_attack(attack,
            |first, second| sides[first - 1].is_enemy(&sides[second - 1]), &mut self.rng, display)?;
        self.fire_units_event("attack_end", from, Some(to))?;
        if let Some(lesson) = &mut self.scenario.events.wounded
            && let (Some(attacker), Some(defender)) = (self.scenario.board.unit_at(from), self.scenario.board.unit_at(to))
        {
            let lines = lesson.after_attack(attacker, defender, prior_experience);
            for line in lines {
                self.queue_dialogue(line, [Some(attacker_id), Some(defender_id)]);
            }
        }
        for (victim, killer, serial, killer_serial, hp, killer_hp, strike) in [
            (to, from, defender_id, attacker_id, result.defender_hitpoints, result.attacker_hitpoints, Some(weapon)),
            (from, to, attacker_id, defender_id, result.attacker_hitpoints, result.defender_hitpoints, retaliation),
        ] {
            if hp > 0 || self.scenario.board.unit_at(victim).is_none_or(|unit| unit.serial != serial || unit.hitpoints > 0) { continue; }
            self.fire_units_event("last_breath", victim, Some(killer))?;
            if self.scenario.board.unit_at(victim).is_none_or(|unit| unit.serial != serial || unit.hitpoints > 0) { continue; }
            self.fire_units_event("die", victim, Some(killer))?;
            let board = &mut self.scenario.board;
            if board.unit_at(victim).is_none_or(|unit| unit.serial != serial || unit.hitpoints > 0) { continue; }
            let replacement = strike.filter(|_| killer_hp > 0 && board.unit_at(killer).is_some_and(|unit| unit.serial == killer_serial)).and_then(|index|
                board.plague_reanimation(killer, victim, index, self.rules.terrain));
            board.remove_casualty(victim);
            if let Some(raised) = replacement {
                board.place_plague_unit(raised.side, raised.base, &raised.variation, victim, &mut self.rng);
            }
        }
        Ok(true)
    }
}

impl<'rules> Battlefield<'rules> {
    fn plague_reanimation(
        &self, from: Hex, to: Hex, weapon: usize, terrain: &TerrainAliases,
    ) -> Option<Reanimation<'rules>> {
        let killer = self.unit_at(from)?;
        let spawn = killer.unit_type.attacks.get(weapon)?.plague_type()?;
        let victim = self.unit_at(to)?;
        let variation = victim.undead_variation.as_deref().unwrap_or(&victim.unit_type.undead_variation);
        if variation == "null" || victim.is_unplagueable()
            || terrain.features(self.map().terrain_at(to)?).village
        {
            return None;
        }
        let base = self.unit_type(if spawn.is_empty() { &killer.unit_type.id } else { spawn })?;
        Some(Reanimation { side: killer.side, base, variation: variation.to_owned() })
    }
}

#[cfg(test)]
mod tests {
    use crate::{board::AttackChoice, fixed_match::{Action, MatchState}, scenario_bundle::Bundle};
    use super::*;

    fn scenario(victim: &str, death_terrain: &str, retaliation: bool, special_type: &str, events: &str) -> Bundle {
        let (west, east) = if retaliation { ("Victim", "Soulless") } else { ("Soulless", "Victim") };
        let (west_tile, east_tile) = if retaliation { (death_terrain, "Gg") } else { ("Gg", death_terrain) };
        let (west_extra, east_extra) = if retaliation { (victim, "") } else { ("", victim) };
        let text = format!(r#"
[terrain_type]
    id=flat
    string=Gg
[/terrain_type]
[terrain_type]
    id=village
    string=^Vh
    gives_income=yes
[/terrain_type]
[units]
    [race]
        id=drake
        undead_variation=drake
    [/race]
    [unit_type]
        id=Victim
        race=drake
        hitpoints=3
        level=1
        [attack]
            name=fist
            type=impact
            range=melee
            damage=1
            number=1
            accuracy=100
        [/attack]
    [/unit_type]
    [unit_type]
        id=Soulless
        hitpoints=30
        [attack]
            name=touch
            type=impact
            range=melee
            damage=8
            number=1
            accuracy=100
            [specials]
                [plague]
                    id=plague
                    {special_type}
                [/plague]
            [/specials]
        [/attack]
        [variation]
            variation_id=drake
            inherit=yes
            hitpoints=39
        [/variation]
    [/unit_type]
    [unit_type]
        id=Walking Corpse
        hitpoints=18
        num_traits=2
        [trait]
            id=undead
            availability=musthave
            [effect]
                apply_to=status
                add=unplagueable
            [/effect]
        [/trait]
        [trait]
            id=quick
        [/trait]
        [variation]
            variation_id=drake
            inherit=yes
            hitpoints=23
        [/variation]
    [/unit_type]
[/units]
[scenario]
    map_data="Xv,Xv,Xv,Xv
Xv,1 {west_tile},2 {east_tile},Xv
Xv,Xv,Xv,Xv"
    [side]
        side=1
        team_name=west
        type={west}
        {west_extra}
    [/side]
    [side]
        side=2
        team_name=east
        type={east}
        {east_extra}
    [/side]
    {events}
[/scenario]"#);
        Bundle::parse(text.as_bytes()).unwrap()
    }

    const WEST: Hex = Hex { x: 0, y: 0 };
    const EAST: Hex = Hex { x: 1, y: 0 };

    fn fight(game: &mut super::super::FixedMatch<'_>) -> MatchState {
        game.step(Action::Attack(AttackChoice { from: WEST, to: EAST, weapon: 0 })).unwrap()
    }

    #[test]
    fn visible_strikes_survive_casualty_removal_and_a_blind_viewer_gets_no_combat() {
        let bundle = scenario("id=fallen", "Gg", false, "type=Walking Corpse",
            "[side]\nside=3\ncontroller=null\nno_leader=yes\nteam_name=neutral\nfog=yes\nshroud=yes\n[/side]");
        let mut reference = bundle.start(3, |_| unreachable!()).unwrap();
        fight(&mut reference);
        for viewer in [1, 3] {
            let mut game = bundle.start(3, |_| unreachable!()).unwrap();
            let (_, combat) = game.attack_for_display(AttackChoice { from: WEST, to: EAST, weapon: 0 }, None, viewer).unwrap();
            assert_eq!(game.audit_snapshot(), reference.audit_snapshot());
            if viewer == 1 {
                let combat = combat.unwrap();
                assert!(combat.opening.units.iter().any(|u| u.position == EAST && u.type_id == "Victim"));
                assert_eq!(combat.blows.last().unwrap().remaining_hp[1], 0);
            } else {
                assert!(combat.is_none());
            }
        }
    }

    #[test]
    fn last_breath_precedes_die_and_only_runs_for_a_casualty() {
        let events = r#"[event]
name=last breath
[filter]
id=fallen
[/filter]
[filter_second]
type=Soulless
[/filter_second]
[modify_side]
side=1
gold=3
[/modify_side]
[/event]
[event]
name=die
[gold]
side=1
amount=7
[/gold]
[/event]"#;
        for retaliation in [false, true] {
            for (victim, expected) in [("id=fallen", 10), ("id=fallen\nhitpoints=30", 100)] {
                let bundle = scenario(victim, "Gg", retaliation, "type=Walking Corpse", events);
                let mut game = bundle.start(3, |_| unreachable!()).unwrap();
                fight(&mut game);
                assert_eq!(game.audit_snapshot().gold[0], expected);
            }
        }
    }

    #[test]
    fn valid_attack_spends_its_costs_before_a_campaign_handler_removes_the_target() {
        let events = r#"[event]
name=attack
[filter_second]
id=fallen
[/filter_second]
[gold]
side=1
amount=13
[/gold]
[kill]
id=fallen
[/kill]
[/event]"#;
        let bundle = scenario("id=fallen", "Gg", false, "type=Walking Corpse", events);
        let mut game = bundle.start(3, |_| unreachable!()).unwrap();
        let illegal = Action::Attack(AttackChoice { from: EAST, to: WEST, weapon: 0 });
        assert!(game.step(illegal).is_err());
        assert_eq!(game.audit_snapshot().gold[0], 100);
        assert_eq!(fight(&mut game), MatchState::Won(1));
        let actor = game.scenario.board.unit_at(WEST).unwrap();
        assert_eq!((actor.attacks_left(), actor.experience), (0, 0));
        assert!(game.scenario.board.unit_at(EAST).is_none());
        assert_eq!(game.audit_snapshot().gold[0], 113);
    }

    #[test]
    fn plague_uses_the_killing_weapon_on_attack_and_retaliation() {
        for retaliation in [false, true] {
            let bundle = scenario("", "Gg", retaliation, "type=Walking Corpse", "");
            let mut game = bundle.start(3, |_| unreachable!()).unwrap();
            let (side, victim, killer) = if retaliation { (2, WEST, EAST) } else { (1, EAST, WEST) };
            assert_eq!(fight(&mut game), MatchState::Won(side));
            let raised = game.scenario.board.unit_at(victim).unwrap();
            assert_eq!((&*raised.unit_type.id, &*raised.unit_type.variation_id), ("Walking Corpse", "drake"));
            assert_eq!((raised.side, raised.hitpoints, raised.moves_left, raised.attacks_left), (side, 23, 0, 0));
            assert_eq!((raised.experience, raised.can_recruit, raised.is_unplagueable()), (0, false, true));
            assert_eq!(raised.traits().map(|t| t.attribute("id").unwrap()).collect::<Vec<_>>(), ["undead", "quick"]);
            assert_eq!(game.scenario.board.unit_at(killer).unwrap().experience, 8);
            assert_eq!(game.rng.seed_and_calls().1, if retaliation { 3 } else { 2 });
        }
    }

    #[test]
    fn die_uses_the_fallen_units_identity_and_runs_before_plague_on_both_sides() {
        for retaliation in [false, true] {
            let victim_side = if retaliation { 1 } else { 2 };
            let victim = if retaliation { WEST } else { EAST };
            let events = format!(r#"
[event]
    name=die
    [filter]
        id=fallen
        type=Victim
        side={victim_side}
        x,y={victim_side},1
    [/filter]
    [filter_second]
        type=Soulless
    [/filter_second]
    [gold]
        side=1
        amount=17
    [/gold]
    [terrain]
        x,y={victim_side},1
        terrain=Gg^Vh
    [/terrain]
[/event]
[event]
    name=attack end
    [filter_second]
        side=2
    [/filter_second]
    [gold]
        side=1
        amount=3
    [/gold]
[/event]
[event]
    name=attack end
    [filter_second]
        side=1
    [/filter_second]
    [gold]
        side=1
        amount=60
    [/gold]
[/event]
[event]
    name=die
    [filter]
        type=Soulless
    [/filter]
    [gold]
        side=1
        amount=100
    [/gold]
[/event]"#);
            let bundle = scenario("id=fallen", "Gg", retaliation, "type=Walking Corpse", &events);
            let mut game = bundle.start(3, |_| unreachable!()).unwrap();
            assert_eq!(fight(&mut game), MatchState::Won(3 - victim_side));
            assert_eq!(game.audit_snapshot().gold[0], 120);
            assert_eq!(game.scenario.board.map().terrain_at(victim), Some("Gg^Vh"));
            assert!(game.scenario.board.unit_at(victim).is_none());

            let bundle = scenario("id=fallen\nhitpoints=30", "Gg", retaliation, "type=Walking Corpse", &events);
            let mut game = bundle.start(3, |_| unreachable!()).unwrap();
            assert!(matches!(fight(&mut game), MatchState::Playing(_)));
            assert_eq!(game.audit_snapshot().gold[0], 103);
        }
    }

    #[test]
    fn healing_the_dying_primary_unit_cancels_removal_and_plague() {
        let events = "[event]\nname=die\n[heal_unit]\namount=1\n[/heal_unit]\n[/event]";
        for retaliation in [false, true] {
            let bundle = scenario("id=fallen", "Gg", retaliation, "type=Walking Corpse", events);
            let mut game = bundle.start(3, |_| unreachable!()).unwrap();
            assert!(matches!(fight(&mut game), MatchState::Playing(_)));
            let victim = game.scenario.board.unit_at(if retaliation { WEST } else { EAST }).unwrap();
            assert_eq!((victim.id.as_deref(), victim.hitpoints), (Some("fallen"), 1));
        }
    }

    #[test]
    fn plague_obeys_village_immunity_authored_status_and_victim_variation() {
        for (victim, terrain, special, expected) in [
            ("", "Gg^Vh", "type=Walking Corpse", None),
            ("[status]\nunplagueable=yes\n[/status]", "Gg", "type=Walking Corpse", None),
            ("undead_variation=null", "Gg", "type=Walking Corpse", None),
            ("", "Gg", "type=missing", None),
            ("undead_variation=unknown", "Gg", "type=Walking Corpse", Some(("Walking Corpse", 18))),
            ("", "Gg", "", Some(("Soulless", 39))),
        ] {
            let bundle = scenario(victim, terrain, false, special, "");
            let mut game = bundle.start(3, |_| unreachable!()).unwrap();
            fight(&mut game);
            assert_eq!(game.scenario.board.unit_at(EAST).map(|u| (&*u.unit_type.id, u.hitpoints)), expected);
        }
    }
}

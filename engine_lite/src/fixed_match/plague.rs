use crate::{
    board::{Attack, Battlefield},
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
    /// Fire die with the casualty still present, then erase it and resolve plague.
    pub(super) fn apply_scenario_attack(
        &mut self, side: usize, attack: Attack,
    ) -> Result<(), MatchError> {
        let (from, to) = (attack.from, attack.to);
        let weapon = attack.weapon;
        let retaliation = attack.retaliation.as_ref().map(|(index, _)| *index);
        let sides = &self.scenario.sides;
        let result = self.scenario.board.resolve_attack(side, attack,
            |first, second| sides[first - 1].is_enemy(&sides[second - 1]), &mut self.rng)?;
        self.fire_units_event("attack_end", from, Some(to))?;
        for (victim, killer, hp, killer_hp, strike) in [
            (to, from, result.defender_hitpoints, result.attacker_hitpoints, Some(weapon)),
            (from, to, result.attacker_hitpoints, result.defender_hitpoints, retaliation),
        ] {
            if hp > 0 { continue; }
            self.fire_units_event("die", victim, Some(killer))?;
            let board = &mut self.scenario.board;
            let replacement = strike.filter(|_| killer_hp > 0).and_then(|index|
                board.plague_reanimation(killer, victim, index, self.rules.terrain));
            board.remove_casualty(victim);
            if let Some(raised) = replacement {
                board.place_plague_unit(raised.side, raised.base, &raised.variation, victim, &mut self.rng);
            }
        }
        Ok(())
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

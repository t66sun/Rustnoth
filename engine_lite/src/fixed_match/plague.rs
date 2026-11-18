use crate::{
    board::{Attack, AttackError, Battlefield},
    combat::CombatResult,
    hex::Hex,
    rng::ReplayRng,
    terrain_aliases::TerrainAliases,
    unit_type::UnitType,
};

struct Reanimation<'rules> {
    side: usize,
    base: &'rules UnitType,
    variation: String,
}

impl<'rules> Battlefield<'rules> {
    /// Applies terrain-dependent plague to either side's lethal strike.
    pub(super) fn apply_scenario_attack(
        &mut self, side: usize, attack: Attack, enemies: impl Fn(usize, usize) -> bool,
        terrain: &TerrainAliases, rng: &mut ReplayRng,
    ) -> Result<CombatResult, AttackError> {
        let (from, to) = (attack.from, attack.to);
        let on_defender_death = self.plague_reanimation(from, to, attack.weapon, terrain);
        let on_attacker_death = attack.retaliation.as_ref().and_then(|(response, _)|
            self.plague_reanimation(to, from, *response, terrain)
        );
        let result = self.apply_attack_with_relations(side, attack, enemies, rng)?;
        let replacement = if result.defender_hitpoints == 0 && result.attacker_hitpoints > 0 {
            on_defender_death.map(|raised| (to, raised))
        } else if result.attacker_hitpoints == 0 && result.defender_hitpoints > 0 {
            on_attacker_death.map(|raised| (from, raised))
        } else {
            None
        };
        if let Some((position, raised)) = replacement {
            self.place_plague_unit(raised.side, raised.base, &raised.variation, position, rng);
        }
        Ok(result)
    }

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

    fn scenario(victim: &str, death_terrain: &str, retaliation: bool, special_type: &str) -> Bundle {
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
            let bundle = scenario("", "Gg", retaliation, "type=Walking Corpse");
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
    fn plague_obeys_village_immunity_authored_status_and_victim_variation() {
        for (victim, terrain, special, expected) in [
            ("", "Gg^Vh", "type=Walking Corpse", None),
            ("[status]\nunplagueable=yes\n[/status]", "Gg", "type=Walking Corpse", None),
            ("undead_variation=null", "Gg", "type=Walking Corpse", None),
            ("", "Gg", "type=missing", None),
            ("undead_variation=unknown", "Gg", "type=Walking Corpse", Some(("Walking Corpse", 18))),
            ("", "Gg", "", Some(("Soulless", 39))),
        ] {
            let bundle = scenario(victim, terrain, false, special);
            let mut game = bundle.start(3, |_| unreachable!()).unwrap();
            fight(&mut game);
            assert_eq!(game.scenario.board.unit_at(EAST).map(|u| (&*u.unit_type.id, u.hitpoints)), expected);
        }
    }
}

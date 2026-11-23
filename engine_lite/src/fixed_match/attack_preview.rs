//! Public strike values for the active side's weapon selection window.
use super::*;
use crate::combat::Combatant;

#[derive(Clone)]
pub struct AttackPreview {
    participants: (u64, u64),
    pub choice: AttackChoice,
    pub offense: StrikePreview,
    pub response: Option<StrikePreview>,
}

#[derive(Clone)]
pub struct StrikePreview {
    pub weapon: usize,
    pub range: String,
    pub damage_type: String,
    pub damage: u32,
    pub strikes: u32,
    pub hit_chance: u8,
}

impl AttackPreview {
    /// A movement event can replace either participant before the selected weapon is used.
    pub fn matches_current(&self, game: &FixedMatch<'_>) -> bool {
        game.attack_previews(self.choice.from, self.choice.to).iter()
            .any(|row| row.choice == self.choice && row.participants == self.participants)
    }
}

impl StrikePreview {
    fn new(weapon: usize, profile: &WeaponProfile, stats: Combatant) -> Self {
        Self {
            weapon,
            range: profile.range.clone(),
            damage_type: profile.damage_type.clone(),
            damage: if stats.slowed {
                stats.slow_damage
            } else {
                stats.damage
            },
            strikes: stats.strikes,
            hit_chance: stats.hit_chance,
        }
    }
}

impl FixedMatch<'_> {
    /// Uses legal targets and visible providers of leadership, illumination and flanking.
    pub fn attack_previews(&self, from: Hex, to: Hex) -> Vec<AttackPreview> {
        self.attack_previews_from(from, from, to)
    }

    /// Preview from a reachable attack hex using only the current side's observation.
    /// Moving the attacker here is hypothetical: visibility, events and movement points stay untouched.
    pub fn attack_previews_from(&self, origin: Hex, from: Hex, to: Hex) -> Vec<AttackPreview> {
        let MatchState::Playing(turn) = self.state else {
            return Vec::new();
        };
        let Some((view, legal)) = self.decision() else {
            return Vec::new();
        };
        let visible: BTreeSet<_> = view.units.iter().map(|unit| unit.position).collect();
        let Some(attacker) = self.scenario.board.unit_at(origin).filter(|unit|
            visible.contains(&origin) && unit.side == view.side && unit.attacks_left() > 0) else { return Vec::new() };
        let Some(defender) = self.scenario.board.unit_at(to).filter(|unit|
            visible.contains(&to) && !view.allied_sides.contains(&unit.side) && !unit.petrified) else { return Vec::new() };
        if origin != from && (from.distance(to) != 1 || visible.contains(&from) ||
            !legal.contains(&Action::Move { from: origin, to: from })) { return Vec::new(); }

        let occupant = |at| {
            if at == from { Some(origin) }
            else { (at != origin && visible.contains(&at)).then_some(at) }
        };
        attacker.unit_type.attacks.iter().enumerate()
            .filter_map(|(weapon_index, profile)| {
                let choice = AttackChoice { from, to, weapon: weapon_index };
                if !within_range(profile, from.distance(to) as i64) ||
                    origin == from && !legal.contains(&Action::Attack(choice)) { return None; }
                let (attack, backstab) = self.prepare_attack_in_view(turn, choice, occupant).ok()?;
                let weapon = &attacker.unit_type.attacks[choice.weapon];
                let parry = attack
                    .retaliation
                    .as_ref()
                    .map_or(0, |(i, _)| defender.unit_type.attacks[*i].parry);
                let offense = StrikePreview::new(
                    choice.weapon,
                    weapon,
                    combatant(
                        attacker,
                        choice.weapon,
                        &attack.modifiers,
                        parry,
                        StrikeRole::Attack { backstab },
                    ),
                );
                let response = attack.retaliation.map(|(i, modifiers)| {
                    StrikePreview::new(
                        i,
                        &defender.unit_type.attacks[i],
                        combatant(
                            defender,
                            i,
                            &modifiers,
                            weapon.parry,
                            StrikeRole::Retaliate(weapon),
                        ),
                    )
                });
                Some(AttackPreview {
                    participants: (attacker.serial, defender.serial),
                    choice,
                    offense,
                    response,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario_bundle::Bundle;

    #[test]
    fn attack_rows_preserve_weapon_indices_and_show_only_eligible_retaliation() {
        let source = br#"
[units]
[unit_type]
id=Archer
hitpoints=24
[attack]
name=knife
damage=3
number=2
range=melee
[/attack]
[attack]
name=bow
damage=5
number=3
range=ranged
[/attack]
[/unit_type]
[unit_type]
id=Guard
hitpoints=36
[attack]
name=spear
damage=4
number=2
range=melee
[/attack]
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
side=1
random_traits=no
type=Archer
[/side]
[side]
side=2
random_traits=no
type=Guard
[/side]
[/scenario]"#;
        let bundle = Bundle::parse(source).unwrap();
        let game = bundle.start(3, |_| unreachable!()).unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        let choices = game.attack_previews(from, to);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].response.as_ref().unwrap().weapon, 0);
        assert_eq!(
            (
                choices[1].choice.weapon,
                choices[1].offense.damage,
                choices[1].offense.strikes
            ),
            (1, 5, 3)
        );
        assert!(choices[1].response.is_none());
        assert!(game.attack_previews(to, from).is_empty());
    }
}

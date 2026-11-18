use super::Battlefield;
use crate::hex::Hex;

impl Battlefield<'_> {
    /// Whether a unit on a hex already in the viewer's sight can be observed.
    /// The lawful bonus is the time of day at this hex. A detector may belong
    /// to any enemy of the hidden unit, including a third side.
    pub fn unit_visible_on_known_hex(
        &self,
        viewer: usize,
        position: Hex,
        lawful_bonus: i32,
        enemies: impl Fn(usize, usize) -> bool,
    ) -> bool {
        let Some(unit) = self.unit_at(position) else { return false };
        if unit.side == viewer || !enemies(viewer, unit.side) || unit.unit_type.hiding.is_empty() {
            return true;
        }
        let terrain = self.map.terrain_at(position).expect("unit on a playable hex");
        let resting = self.resting.contains(&position);
        !unit.unit_type.hiding.iter().any(|ability| ability.applies(terrain, lawful_bonus, resting))
            || position.neighbors().filter_map(|hex| self.unit_at(hex))
                .any(|neighbor| enemies(unit.side, neighbor.side))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{map::ScenarioMap, unit_type::read_unit_types, wml};

    const HIDING: Hex = Hex { x: 1, y: 0 };
    const NEIGHBOR: Hex = Hex { x: 2, y: 0 };

    fn observes(terrain: &str, filter: &str, formula: &str, lawful: i32, resting: bool) -> bool {
        let types = read_unit_types(wml::parse(&format!(
            "[unit_type]\nid=Hider\n[abilities]\n[hides]\n[filter]\n{formula}\n\
             [filter_location]\n{filter}\n[/filter_location]\n[/filter]\n[/hides]\n[/abilities]\n[/unit_type]"
        )).unwrap());
        let map = ScenarioMap::parse(&format!(
            "Xv,Xv,Xv,Xv,Xv\nXv,Gg,{terrain},Gg,Xv\nXv,Xv,Xv,Xv,Xv"
        )).unwrap();
        let mut board = Battlefield::new(map, vec![], &types).unwrap();
        board.place_recruit(2, "Hider", HIDING).unwrap();
        if resting { board.begin_resting(2); }
        board.unit_visible_on_known_hex(1, HIDING, lawful, |a, b| a != b)
    }

    #[test]
    fn stock_hiding_filters_follow_the_current_hex_and_turn() {
        for (filter, formula, hex, lawful, resting, visible) in [
            ("terrain=*^F*,*^Qhhf,*^Qhuf", "", "Gg^Fds", 0, false, false),
            ("terrain=*^F*,*^Qhhf,*^Qhuf", "", "Qxu^Qhhf", 0, false, false),
            ("terrain=*^F*,*^Qhhf,*^Qhuf", "", "Qxu^Qhuf", 0, false, false),
            ("terrain=*^F*,*^Qhhf,*^Qhuf", "", "Gg", 0, false, true),
            ("terrain=Wo*^*", "", "Wog", 0, false, false),
            ("terrain=Wo*^*", "", "Ww^Bsb", 0, false, true),
            ("terrain=*^V*", "", "Hh^Vhh", 0, false, false),
            ("terrain=S*^*", "", "Ss", 0, false, false),
            ("time_of_day=chaotic", "", "Gg", -25, false, false),
            ("time_of_day=chaotic", "", "Gg", 0, false, true),
            ("terrain=*^F*,*^Qhhf,*^Qhuf,D*^*", "formula=self.resting", "Ds", 0, true, false),
            ("terrain=*^F*,*^Qhhf,*^Qhuf,D*^*", "formula=self.resting", "Ds", 0, false, true),
            ("time_of_day_id=custom", "", "Gg", 25, false, false),
        ] {
            assert_eq!(observes(hex, filter, formula, lawful, resting), visible, "{filter} {hex}");
        }
    }

    #[test]
    fn an_enemy_of_the_hider_exposes_it_to_other_sides() {
        let types = read_unit_types(wml::parse(
            "[unit_type]\nid=Hider\n[abilities]\n[hides]\nid=hidden\n[/hides]\n[/abilities]\n[/unit_type]"
        ).unwrap());
        let map = ScenarioMap::parse("Xv,Xv,Xv,Xv,Xv\nXv,Gg,Gg,Gg,Xv\nXv,Xv,Xv,Xv,Xv").unwrap();
        let mut board = Battlefield::new(map, vec![], &types).unwrap();
        board.place_recruit(2, "Hider", HIDING).unwrap();
        let enemies = |a, b| a != b && (a == 2 || b == 2) && a != 4 && b != 4;
        assert!(board.unit_visible_on_known_hex(2, HIDING, 0, enemies));
        assert!(board.unit_visible_on_known_hex(4, HIDING, 0, enemies));
        assert!(!board.unit_visible_on_known_hex(1, HIDING, 0, enemies));
        board.place_recruit(3, "Hider", NEIGHBOR).unwrap();
        assert!(board.unit_visible_on_known_hex(1, HIDING, 0, enemies));
    }
}

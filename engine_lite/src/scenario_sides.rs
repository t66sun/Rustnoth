use std::collections::BTreeSet;

use crate::{hex::Hex, wml::Node};

/// Which parts of this side's visibility are shared with allies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShareVision {
    All,
    Shroud,
    None,
}

/// WML criterion used to decide a side's survival and village ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefeatCondition {
    NoLeaderLeft,
    NoUnitsLeft,
    Never,
    Always,
}

/// Literal scenario objectives shown by the desktop client.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScenarioObjectives {
    pub victory: Vec<String>,
    pub defeat: Vec<String>,
}

impl ScenarioObjectives {
    pub(crate) fn read(cfg: &Node) -> Self {
        let mut result = Self::default();
        for (_, objective) in cfg.children.iter().filter(|(tag, _)| tag == "objective") {
            if objective.children.iter().any(|(tag, _)| tag == "show_if") { continue; }
            let Some(description) = objective.attribute("description")
                .filter(|text| !text.trim().is_empty() && !text.contains('$')) else { continue };
            match objective.attribute("condition") {
                Some("win") => result.victory.push(description.to_owned()),
                Some("lose") => result.defeat.push(description.to_owned()),
                _ => {}
            }
        }
        result
    }

    pub fn is_empty(&self) -> bool {
        self.victory.is_empty() && self.defeat.is_empty()
    }
}

/// The initial rules settings for a side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SideConfig {
    /// One-based position in the scenario's side roster.
    pub side: usize,
    pub team_names: Vec<String>,
    pub(crate) objectives: ScenarioObjectives,
    pub(crate) defeat_condition: DefeatCondition,
    pub fog: bool,
    pub shroud: bool,
    pub(crate) fog_data: String,
    pub(crate) fog_override: Option<(String, String)>,
    pub(crate) shroud_data: String,
    pub share_vision: ShareVision,
    pub starting_gold: i32,
    /// The side's WML `income`, added to the global base income.
    pub income_bonus: i32,
    pub village_gold: i32,
    pub village_support: i32,
    pub recall_cost: i32,
    pub recruits: BTreeSet<String>,
    /// Authored initial village locations, to be checked against the map.
    pub starting_villages: BTreeSet<Hex>,
}

impl SideConfig {
    /// Distinct sides are allied when they share at least one team name.
    pub fn is_enemy(&self, other: &Self) -> bool {
        self.side != other.side
            && !self.team_names.iter().any(|name| other.team_names.contains(name))
    }
}

/// Reads direct `[side]` children of a preprocessed scenario in roster order.
/// Omitted economy settings use the standard Wesnoth 1.18.8 scenario defaults.
pub fn read_sides(scenario: &Node) -> Vec<SideConfig> {
    scenario
        .children
        .iter()
        .filter(|(tag, _)| tag == "side")
        .enumerate()
        .map(|(index, (_, node))| read_side(index + 1, node))
        .collect()
}

fn read_side(side: usize, cfg: &Node) -> SideConfig {
    let team_names = cfg
        .attribute("team_name")
        .filter(|name| !name.is_empty())
        .or_else(|| cfg.attribute("side"))
        .unwrap_or_default();
    let gold = cfg.attribute("gold").unwrap_or_default();
    let starting_gold = if gold.is_empty() {
        100
    } else {
        int_attribute(cfg, "gold", 0)
    };

    SideConfig {
        side,
        team_names: names(team_names).collect(),
        objectives: ScenarioObjectives::default(),
        defeat_condition: match cfg.attribute("defeat_condition") {
            Some("no_units_left") => DefeatCondition::NoUnitsLeft,
            Some("never") => DefeatCondition::Never,
            Some("always") => DefeatCondition::Always,
            _ => DefeatCondition::NoLeaderLeft,
        },
        fog: bool_attribute(cfg, "fog", false),
        shroud: bool_attribute(cfg, "shroud", false),
        fog_data: cfg.attribute("fog_data").unwrap_or_default().to_owned(),
        fog_override: cfg.children.iter().find(|(tag, _)| tag == "fog_override")
            .map(|(_, locations)| (
                locations.attribute("x").unwrap_or_default().to_owned(),
                locations.attribute("y").unwrap_or_default().to_owned(),
            )),
        shroud_data: cfg.attribute("shroud_data").unwrap_or_default().to_owned(),
        share_vision: read_share_vision(cfg),
        starting_gold,
        income_bonus: int_attribute(cfg, "income", 0),
        village_gold: int_attribute(cfg, "village_gold", 1),
        village_support: int_attribute(cfg, "village_support", 1),
        recall_cost: int_attribute(cfg, "recall_cost", 20),
        recruits: names(cfg.attribute("recruit").unwrap_or_default()).collect(),
        starting_villages: cfg
            .children
            .iter()
            .filter(|(tag, _)| tag == "village")
            .filter_map(|(_, village)| {
                let x = int_attribute(village, "x", 0);
                let y = int_attribute(village, "y", 0);
                Hex::from_wml(x, y).filter(|hex| hex.x >= 0 && hex.y >= 0)
            })
            .collect(),
    }
}

pub(crate) fn names(list: &str) -> impl Iterator<Item = String> + '_ {
    list.split(',')
        .map(str::trim_ascii)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn read_share_vision(cfg: &Node) -> ShareVision {
    if cfg.attributes.contains_key("share_view") || cfg.attributes.contains_key("share_maps") {
        if bool_attribute(cfg, "share_view", false) {
            return ShareVision::All;
        }
        if bool_attribute(cfg, "share_maps", true) {
            return ShareVision::Shroud;
        }
        return ShareVision::None;
    }

    match cfg.attribute("share_vision") {
        Some("shroud") => ShareVision::Shroud,
        Some("none") => ShareVision::None,
        _ => ShareVision::All,
    }
}

pub(crate) fn bool_attribute(cfg: &Node, key: &str, default: bool) -> bool {
    match cfg.attributes.get(key) {
        Some(value) if value.translations.is_empty() => match value.text.as_str() {
            "yes" | "true" => true,
            "no" | "false" => false,
            _ => default,
        },
        _ => default,
    }
}

fn int_attribute(cfg: &Node, key: &str, default: i32) -> i32 {
    let Some(value) = cfg.attributes.get(key).filter(|v| v.translations.is_empty()) else {
        return default;
    };
    let text = value.text.trim_ascii_start();
    // Wesnoth's string-to-integer conversion consumes the leading decimal digits.
    let sign = usize::from(text.starts_with(['+', '-']));
    let digit_count = text.as_bytes()[sign..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    text[..sign + digit_count].parse().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    #[test]
    fn reads_the_authored_village_locations_of_each_side() {
        let scenario = wml::parse(
            "[side]\n[village]\nx,y=4,2\n[/village]\n[village]\nx,y=4,2\n[/village]\n\
             [village]\nx=8\n[/village]\n[/side]\n[side]\n[village]\nx,y=1,1\n[/village]\n[/side]",
        )
        .unwrap();
        let sides = read_sides(&scenario);
        assert_eq!(sides[0].starting_villages, BTreeSet::from([Hex { x: 3, y: 1 }]));
        assert_eq!(sides[1].starting_villages, BTreeSet::from([Hex { x: 0, y: 0 }]));
    }

    #[test]
    fn alliances_require_a_shared_team_name_between_the_two_sides() {
        let scenario = wml::parse(
            "[side]\nteam_name=east\n[/side]\n\
             [side]\nteam_name=east, west\n[/side]\n\
             [side]\nteam_name=west\n[/side]\n\
             [side]\n[/side]\n",
        )
        .unwrap();
        let sides = read_sides(&scenario);
        assert!(!sides[0].is_enemy(&sides[1]));
        assert!(!sides[1].is_enemy(&sides[2]));
        assert!(sides[0].is_enemy(&sides[2]));
        assert!(sides[2].is_enemy(&sides[0]));
        assert!(sides[0].is_enemy(&sides[3]));
        assert!(!sides[3].is_enemy(&sides[3]));
    }

    #[test]
    fn keeps_side_order_and_falls_back_to_the_declared_team_name() {
        let scenario = wml::parse(
            r#"
            [side]
                side=1
                team_name=" Loyalists, Convoy, "
                fog=true
                shroud=yes
                recruit="Spearman, Heavy Infantryman, Spearman, , Bowman"
            [/side]
            [event]
                [side]
                    side=30
                [/side]
            [/event]
            [side]
                side=2
            [/side]
            "#,
        )
        .unwrap();

        let sides = read_sides(&scenario);
        assert_eq!(sides.len(), 2);
        assert_eq!((sides[0].side, sides[1].side), (1, 2));
        assert_eq!(sides[0].team_names, ["Loyalists", "Convoy"]);
        assert_eq!(sides[1].team_names, ["2"]);
        assert!(sides[0].fog && sides[0].shroud);
        assert!(!sides[1].fog && !sides[1].shroud);
        assert_eq!(
            sides[0].recruits.iter().map(String::as_str).collect::<Vec<_>>(),
            ["Bowman", "Heavy Infantryman", "Spearman"]
        );
    }

    #[test]
    fn loads_fresh_scenario_economy_defaults_and_explicit_values() {
        let scenario = wml::parse(
            r#"
            [side]
                side=1
            [/side]
            [side]
                side=2
                gold=0
                income=-2
                village_gold=4
                village_support=0
                recall_cost=10
            [/side]
            [side]
                side=3
                gold=-5
                village_gold=all
            [/side]
            "#,
        )
        .unwrap();
        let sides = read_sides(&scenario);
        let economy = |side: &SideConfig| {
            (
                side.starting_gold,
                side.income_bonus,
                side.village_gold,
                side.village_support,
                side.recall_cost,
            )
        };
        assert_eq!(economy(&sides[0]), (100, 0, 1, 1, 20));
        assert_eq!(economy(&sides[1]), (0, -2, 4, 0, 10));
        assert_eq!(economy(&sides[2]), (-5, 0, 1, 1, 20));
    }

    #[test]
    fn applies_native_legacy_visibility_precedence() {
        let scenario = wml::parse(
            r#"
            [side]
                share_vision=none
                share_view=yes
                share_maps=no
                fog=1
            [/side]
            [side]
                share_vision=none
                share_view=no
            [/side]
            [side]
                share_vision=shroud
                share_maps=no
            [/side]
            [side]
                share_vision=shroud
            [/side]
            "#,
        )
        .unwrap();

        let sides = read_sides(&scenario);
        assert!(!sides[0].fog);
        assert_eq!(sides[0].share_vision, ShareVision::All);
        assert_eq!(sides[1].share_vision, ShareVision::Shroud);
        assert_eq!(sides[2].share_vision, ShareVision::None);
        assert_eq!(sides[3].share_vision, ShareVision::Shroud);
    }
}

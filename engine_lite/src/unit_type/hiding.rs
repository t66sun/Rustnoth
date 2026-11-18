use crate::wml::Node;

/// Conditions used by the stock self-concealment abilities.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Hiding {
    Forest,
    Burrow,
    Village,
    DeepWater,
    Swamp,
    Night,
    UnconditionalOrUnknown,
}

impl Hiding {
    pub(super) fn from_wml(ability: &Node) -> Self {
        let [(tag, filter)] = ability.children.as_slice() else {
            return Self::UnconditionalOrUnknown;
        };
        let [(location_tag, location)] = filter.children.as_slice() else {
            return Self::UnconditionalOrUnknown;
        };
        if tag != "filter" || location_tag != "filter_location" || !location.children.is_empty()
            || location.attributes.len() != 1
            || filter.attributes.keys().any(|key| key != "formula")
        {
            return Self::UnconditionalOrUnknown;
        }
        match (location.attribute("terrain"), location.attribute("time_of_day"), filter.attribute("formula")) {
            (Some("*^F*,*^Qhhf,*^Qhuf"), None, None) => Self::Forest,
            (Some("*^F*,*^Qhhf,*^Qhuf,D*^*"), None, Some("self.resting")) => Self::Burrow,
            (Some("*^V*"), None, None) => Self::Village,
            (Some("Wo*^*"), None, None) => Self::DeepWater,
            (Some("S*^*"), None, None) => Self::Swamp,
            (None, Some("chaotic"), None) => Self::Night,
            _ => Self::UnconditionalOrUnknown,
        }
    }

    pub(crate) fn applies(&self, terrain: &str, lawful_bonus: i32, resting: bool) -> bool {
        let (base, overlay) = terrain.split_once('^').unwrap_or((terrain, ""));
        let forest = overlay.starts_with('F') || matches!(overlay, "Qhhf" | "Qhuf");
        match self {
            Self::Forest => forest,
            Self::Burrow => resting && (forest || base.starts_with('D')),
            Self::Village => overlay.starts_with('V'),
            Self::DeepWater => base.starts_with("Wo"),
            Self::Swamp => base.starts_with('S'),
            Self::Night => lawful_bonus < 0,
            Self::UnconditionalOrUnknown => true,
        }
    }
}

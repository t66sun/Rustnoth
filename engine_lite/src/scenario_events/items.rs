use std::collections::BTreeMap;

use crate::{
    hex::Hex,
    scenario_sides::{bool_attribute, names},
    visibility::{HexVisibility, VisibilityMask},
    wml::Node,
};

#[derive(Clone, Debug)]
pub(super) struct MapItem {
    image: String,
    name: Option<String>,
    teams: Vec<String>,
    visible_in_fog: bool,
    z_order: f64,
}

#[derive(Clone, Debug)]
pub(super) enum ItemChange {
    Place(Hex, MapItem),
    Remove(Hex, Option<String>),
}

impl ItemChange {
    pub(super) fn read(tag: &str, cfg: &Node) -> Option<Self> {
        if !cfg.children.is_empty() || cfg.attributes.values().any(|v| {
            !v.translations.is_empty() || v.text.contains('$')
        }) { return None; }
        let x = cfg.attribute("x")?.trim().parse().ok()?;
        let y = cfg.attribute("y")?.trim().parse().ok()?;
        let hex = Hex::from_wml(x, y)?;
        match tag {
            "item" if cfg.attributes.keys().all(|key| matches!(key.as_str(),
                "x" | "y" | "image" | "name" | "visible_in_fog" | "team_name" | "z_order" | "redraw"
            )) => {
                let z_order = cfg.attribute("z_order").unwrap_or("0").trim().parse::<f64>().ok()?;
                if !z_order.is_finite() { return None; }
                Some(Self::Place(hex, MapItem {
                    image: cfg.attribute("image")?.to_owned(),
                    name: cfg.attribute("name").map(str::to_owned),
                    teams: names(cfg.attribute("team_name").unwrap_or_default()).collect(),
                    visible_in_fog: bool_attribute(cfg, "visible_in_fog", true),
                    z_order,
                }))
            }
            "remove_item" if cfg.attributes.keys().all(|key| matches!(key.as_str(), "x" | "y" | "image")) =>
                Some(Self::Remove(hex, cfg.attribute("image").map(str::to_owned))),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct MapItems {
    overlays: BTreeMap<Hex, Vec<MapItem>>,
    next_item_name: u64,
}

impl MapItems {
    pub(super) fn apply(&mut self, change: &ItemChange) {
        match change {
            ItemChange::Place(hex, item) => {
                let mut item = item.clone();
                if item.name.is_none() {
                    item.name = Some(format!("item_{}", self.next_item_name));
                    self.next_item_name += 1;
                }
                let overlays = self.overlays.entry(*hex).or_default();
                let index = overlays.partition_point(|old| old.z_order <= item.z_order);
                overlays.insert(index, item);
            }
            ItemChange::Remove(hex, Some(name)) => {
                if let Some(items) = self.overlays.get_mut(hex) {
                    items.retain(|i| i.name.as_ref() != Some(name) && i.image != *name);
                    if items.is_empty() { self.overlays.remove(hex); }
                }
            }
            ItemChange::Remove(hex, None) => { self.overlays.remove(hex); }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.overlays.is_empty()
    }

    pub(crate) fn visible_images(&self, vision: &VisibilityMask, teams: &[String]) -> Vec<(Hex, &str)> {
        let mut images = Vec::new();
        for (&hex, items) in &self.overlays {
            let Some(vis @ (HexVisibility::Visible | HexVisibility::Fogged)) = vision.at(hex) else { continue };
            for item in items {
                if vis == HexVisibility::Fogged && !item.visible_in_fog { continue; }
                if item.teams.is_empty() || item.teams.iter().any(|t| teams.contains(t)) {
                    images.push((hex, item.image.as_str()));
                }
            }
        }
        images
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_match::Action, scenario_bundle::Bundle};

    #[test]
    fn campaign_items_are_ordered_removed_and_filtered_for_each_viewer() {
        let config = br#"[units]
[unit_type]
id=Scout
hitpoints=20
movement=0
vision=0
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Scout
team_name=west
fog=yes
[/side]
[side]
type=Scout
team_name=east
shroud=yes
[/side]
[event]
name=prestart
[item]
x,y=3,1
image=items/ring.png
z_order=5
[/item]
[item]
x,y=3,1
image=items/chest.png
[/item]
[item]
x,y=3,1
image=items/altar.png
visible_in_fog=no
[/item]
[item]
x,y=3,1
image=items/book.png
team_name=east
[/item]
[item]
x,y=3,1
image=items/sword.png
name=blade
[/item]
[/event]
[event]
name=start
[remove_item]
x,y=3,1
image=blade
[/remove_item]
[/event]
[event]
name=side 2 turn
[remove_item]
x,y=3,1
image=items/chest.png
[/remove_item]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(config).unwrap();
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let location = Hex { x: 2, y: 0 };
        assert_eq!(game.map_item_images(1), [(location, "items/chest.png"), (location, "items/ring.png")]);
        assert!(game.map_item_images(2).is_empty());
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.map_item_images(1), [(location, "items/ring.png")]);
    }
}

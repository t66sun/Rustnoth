//! Generate unit articles from the public type catalog and the player's observation.
use std::{collections::BTreeSet, path::Path};
use engine_lite::{observation::Observation, scenario_bundle::Bundle, time_of_day::Alignment, unit_type::UnitType};
use rustnoth_gui::{assets::AssetCatalog, team_colors::TeamColors};
use crate::{BoardData, title_screen::{ButtonBacking, locale::Locale, title_fonts}};
use super::*;

fn topic(root: &Path, assets: &AssetCatalog, rules: &Bundle, unit: &UnitType,
    locale: &mut Locale, terrain: &BTreeSet<&str>) -> Topic {
    let id = &unit.id;
    let text = assets.unit_help_text(id);
    let title = text.map_or_else(|| id.clone(), |t| locale.attribute(root, t, "name", id));
    let description = text.map_or_else(String::new, |t| locale.attribute(root, t, "description", ""));
    let label = |s| locale.game(s);
    let mut spans = vec![Span::Text(format!("{} {}\n\n", label("Level"), unit.level)),
        Span::Image(format!("unit-image:{id}"))];
    let mut lines = Vec::new();
    if !unit.advances_to.is_empty() {
        let names = unit.advances_to.iter().map(|id| assets.unit_name(id).unwrap_or(id)).collect::<Vec<_>>().join(", ");
        lines.push(format!("{}{names}", label("Advances to: ")));
    }
    if let Some(race) = assets.unit_race_name(id) { lines.push(format!("{} {race}", label("Race:"))); }
    let abilities = assets.unit_ability_names(id);
    if !abilities.is_empty() { lines.push(format!("{}{}", label("Abilities: "), abilities.join(", "))); }
    let stats = [("HP:", unit.hitpoints), ("Moves:", unit.movement), ("Cost:", unit.cost), ("XP:", unit.experience)];
    lines.push(stats.map(|(key, n)| format!("{} {n}", label(key))).join("    "));
    let alignment = match unit.alignment {
        Alignment::Lawful => "lawful", Alignment::Neutral => "neutral",
        Alignment::Chaotic => "chaotic", Alignment::Liminal => "liminal",
    };
    lines.push(format!("{} {}", label("Alignment:"), label(alignment)));
    spans.push(Span::Text(format!("{}\n\n{description}", lines.join("\n"))));
    if !unit.attacks.is_empty() {
        let heading = label("unit help^Attacks");
        spans.push(Span::Heading(format!("\n\n{}\n", heading.rsplit('^').next().unwrap_or(heading))));
        for (index, weapon) in unit.attacks.iter().enumerate() {
            let name = assets.weapon_name(id, index).unwrap_or(&weapon.id);
            spans.push(Span::Text(format!("{}    {} × {}    {}    {}\n",
                name, weapon.damage, weapon.strikes, locale.game(&weapon.range), locale.game(&weapon.damage_type))));
        }
    }
    let resistances = rules.unit_resistances(unit);
    if !resistances.is_empty() {
        spans.push(Span::Heading(format!("\n\n{}\n", label("Resistances"))));
        for (kind, percent) in resistances { spans.push(Span::Text(format!("{}    {percent}%\n", locale.game(kind)))); }
    }
    // The terrain help table only includes terrain the viewer has discovered.
    let mut entries = BTreeSet::new();
    for code in terrain {
        let (defense, cost) = rules.unit_terrain_modifiers(unit, code);
        entries.insert((assets.terrain_name(code).unwrap_or(code), defense, cost));
    }
    if !entries.is_empty() {
        spans.push(Span::Heading(format!("\n\n{}\n", label("Terrain Modifiers"))));
        spans.push(Span::Text(format!("{}    {}    {}\n", label("Terrain"), label("Defense"), label("Movement Cost"))));
        for (name, defense, cost) in entries {
            let movement = cost.map(|n| n.to_string()).unwrap_or_else(|| "‒".to_owned());
            spans.push(Span::Text(format!("{name}    {defense}%    {movement}\n")));
        }
    }
    Topic { title, spans }
}

pub(crate) async fn show_unit(data: &BoardData, view: &Observation, selected: &str,
    locale: &mut Locale) -> Result<(), Box<dyn Error>> {
    let root = &data.wesnoth_root;
    let board = crate::dialog::Backdrop::capture(FilterMode::Nearest);
    let mut book = Book::read(&data.core)?;
    let mut textures = HashMap::new();
    let terrain = view.tiles.iter().map(|tile| tile.terrain.as_str()).collect();
    let colors = std::fs::read(root.join("data/core/team-colors.cfg")).ok()
        .and_then(|bytes| TeamColors::parse(&bytes).ok());
    let visible_types: BTreeSet<_> = view.units.iter().map(|u| u.type_id.as_str()).collect();
    for id in visible_types {
        let Some(kind) = data.scenario.unit_type(id) else { continue };
        let sprite = data.assets.unit_image(id).and_then(|path|
            crate::render::load_texture(&path, colors.as_ref().map(|palette| (palette, 1))));
        textures.insert(format!("unit-image:{id}"), sprite);
        book.insert_unit(format!("unit_{id}"), topic(root, &data.assets, &data.scenario, kind, locale, &terrain));
    }
    let current = format!("unit_{selected}");
    if !book.topics.contains_key(&current) { return Err(format!("Missing visible unit type {selected}").into()); }
    let (font, italic) = title_fonts(root, &locale.effective);
    let logo = load_texture(root, &locale.logo(root))?;
    let logo_shadow = load_texture(root, "images/misc/logo-bg.png")?;
    let buttons = ButtonBacking::load(root)?;
    let page = PageArt { font: font.as_ref(), italic: italic.as_ref(), logo: &logo, logo_shadow: &logo_shadow };
    next_frame().await;
    browse(root, book, current, &page, textures,
        |w, h| draw_texture_ex(board.texture(), 0., 0., WHITE, DrawTextureParams {
            dest_size: Some(vec2(w,h)), flip_y: true, ..Default::default()
        }),
        |r, label, hover, pressed| ButtonBacking::button(&buttons, font.as_ref(), r, locale.ui(label), hover, pressed)
    ).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_help_uses_campaign_descriptions_attacks_and_inherited_terrain_rules() {
        let root = Path::new("wesnoth");
        let core = br#"[terrain_type]
id=flat
name=Grassland
string=Gg
[/terrain_type]
[units]
[movetype]
name=foot
[movement_costs]
flat=2
[/movement_costs]
[defense]
flat=60
[/defense]
[resistance]
cold=80
[/resistance]
[/movetype]
[/units]"#;
        let campaign = br#"[scenario]
id=training
[/scenario]
[units]
[unit_type]
id=Dummy
name=Quintain
description=Practice on this dummy.
movement_type=foot
level=1
hitpoints=22
[attack]
name=flail
description=practice flail
range=melee
type=impact
damage=3
number=2
[/attack]
[/unit_type]
[/units]"#;
        let rules = Bundle::parse_with_core(campaign, core).unwrap();
        let assets = AssetCatalog::parse(root, core, campaign).unwrap();
        let topic = topic(root, &assets, &rules, rules.unit_type("Dummy").unwrap(),
            &mut Locale::default(), &BTreeSet::from(["Gg"]));
        assert_eq!(topic.title, "Quintain");
        let text = topic.spans.iter().filter_map(|s| match s { Span::Text(line) => Some(line.as_str()), _ => None })
            .collect::<String>();
        for part in ["HP: 22", "Practice on this dummy.", "practice flail    3 × 2",
            "cold    20%", "Grassland    40%    2"] {
            assert!(text.contains(part), "{part} missing from {text}");
        }
        let mut book = Book::read(core).unwrap();
        book.insert_unit("unit_Dummy".into(), topic);
        assert_eq!(book.rows(&HashSet::new())[0].id, "unit_Dummy");
    }
}

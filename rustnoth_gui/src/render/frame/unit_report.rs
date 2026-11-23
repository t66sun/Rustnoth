//! The default theme shows the visible pointed unit, falling back to the selection.
use super::*;
use engine_lite::{fixed_match::unit_report::UnitReport, observation::ObservedUnit, time_of_day::Alignment};

const IVORY: Color = Color::from_rgba(245, 230, 193, 255);
const DETAILS: Color = Color::from_rgba(196, 176, 147, 255);

pub(super) struct Art {
    pub(super) cached: Option<((Hex, Hex), Option<UnitReport>)>,
    icons: HashMap<String, Texture>,
    conditions: [Texture; 3],
    headings: [String; 4],
    #[cfg(windows)]
    heading_glyphs: [[Texture; 4]; 2],
}

impl Art {
    pub(super) fn new(root: &Path, locale: &crate::title_screen::locale::Locale) -> Self {
        let headings = ["HP", "XP", "MP", "def"].map(|label| locale.game(label).to_owned());
        #[cfg(windows)]
        let font = crate::title_screen::tip_pango::Text::load(root, "Lato");
        Self {
            cached: None,
            icons: HashMap::new(),
            #[cfg(windows)]
            heading_glyphs: [13, 11].map(|size| headings.each_ref().map(|label| {
                font.as_ref().map(|font| {
                    let tex = Texture2D::from_image(&font.button(label, size, [221; 3]));
                    tex.set_filter(FilterMode::Nearest);
                    tex
                })
            })),
            headings,
            conditions: ["slowed", "poisoned", "petrified"]
                .map(|name| load_texture(&root.join("images/misc").join(format!("{name}.png")), None)),
        }
    }

    // Static labels in themes/default.cfg are anchored to unit-box-topright.
    // Only the values below those labels depend on the unit under the pointer.
    pub(super) fn draw_headings(&self, frame: &BattlefieldFrame, font: &Option<Font>) {
        let scale = frame.scale;
        let line = if frame.compact { 13. } else { 14. };
        let left = frame.sidebar.x + 89. * scale;
        let top = frame.minimap_frame.bottom() + 89. * scale;
        let step = (line + 14.) * scale;
        let positions = [(left, top, 60.), (left, top + step, 65.),
            (left, top + 2. * step, 65.), (left + 66. * scale, top + 2. * step, 25.)];
        for (i, (x, y, width)) in positions.into_iter().enumerate() {
            if y + line * scale > frame.end_turn.y - 20. * scale { continue; }
            #[cfg(windows)]
            if let Some(glyphs) = &self.heading_glyphs[usize::from(frame.compact)][i] {
                let ratio = (width / glyphs.width()).min(1.) * scale;
                draw_texture_ex(glyphs, x, y, WHITE, DrawTextureParams {
                    dest_size: Some(glyphs.size() * ratio), ..Default::default()
                });
                continue;
            }
            let size = if frame.compact { 11 } else { 13 };
            text_in(font, &self.headings[i], Rect::new(x, y, width * scale, line * scale),
                size, Color::from_rgba(221, 221, 221, 255), false);
        }
    }

    fn update(&mut self, unit: Hex, pointer: Hex, movement: &MovementView<'_>) {
        if self
            .cached
            .as_ref()
            .is_some_and(|(key, _)| *key == (unit, pointer))
        {
            return;
        }
        let report = movement.session.unit_report(unit, pointer);
        if let Some(report) = &report {
            for weapon in &report.weapons {
                for id in [format!("{}_attack", weapon.range), weapon.damage_type.clone()] {
                    self.icons.entry(id.clone()).or_insert_with(|| {
                        let path = movement
                            .assets
                            .resolve_image(&Path::new("icons/profiles").join(format!("{id}.png")))?;
                        path.is_file().then(|| load_texture(&path, None)).flatten()
                    });
                }
            }
        }
        self.cached = Some(((unit, pointer), report));
    }
}

pub(super) fn displayed_unit(
    view: &Observation,
    hovered: Option<Hex>,
    selected: Option<Hex>,
) -> Option<&ObservedUnit> {
    [hovered, selected]
        .into_iter()
        .flatten()
        .find_map(|hex| view.units.iter().find(|u| u.position == hex))
}

pub(super) fn draw(
    frame: &BattlefieldFrame,
    art: &mut BoardArt,
    unit: &ObservedUnit,
    pointer: Hex,
    movement: &MovementView<'_>,
) {
    art.frame.unit_report.update(unit.position, pointer, movement);
    let Some((_, Some(report))) = &art.frame.unit_report.cached else {
        return;
    };
    let icons = &art.frame.unit_report.icons;
    let s = frame.scale;
    let left = frame.sidebar.x + 7.0 * s;
    let top = frame.minimap_frame.bottom() + 90.0 * s;
    let rect = |x: f32, y: f32, w: f32, h: f32| Rect::new(left + x * s, top + y * s, w * s, h * s);
    let tiny = if frame.compact { 11 } else { 13 };
    let small = if frame.compact { 13 } else { 15 };
    let text = |label: &str, x, y, w, h, size, color| {
        let bounds = rect(x, y, w, h);
        if bounds.bottom() <= frame.end_turn.y - 20.0 * s {
            text_in(&art.font, label, bounds, size, color, false);
        }
    };
    if let Some(Some(sprite)) = art.units.get(&unit.type_id).and_then(|u| u.get(&unit.side)) {
        let cell = rect(3.0, 3.0, 72.0, 72.0);
        super::super::draw_centered_unit(sprite, cell.center(), cell.w, false, None);
    }

    let status = &art.frame.unit_report.conditions;
    let mut next = 140.0;
    for (index, active) in [unit.slowed, unit.poisoned, unit.petrified]
        .into_iter()
        .enumerate()
    {
        if active {
            paint(&status[index], rect(next, -4.0, 16.0, 16.0));
            next += 12.0;
        }
    }

    let rgb = |[r, g, b]: [u8; 3]| Color::from_rgba(r, g, b, 255);
    let bars = movement.assets.unit_bars(&unit.type_id);
    let advancement = bars.and_then(|bars| bars.advancement);
    let experience = if advancement.is_some() {
        format!("{}/{}", unit.experience, unit.max_experience)
    } else {
        "–".into()
    };
    let hp_color =
        super::super::unit_status::hp_color(unit.hitpoints as f32 / unit.max_hitpoints.max(1) as f32);
    let xp_color = super::super::unit_status::xp_color(
        unit.max_experience.saturating_sub(unit.experience),
        advancement == Some(true),
    );
    let line = if frame.compact { 13.0 } else { 14.0 };
    let stats = [
        (
            format!("{}/{}", unit.hitpoints, unit.max_hitpoints),
            rgb(hp_color),
        ),
        (experience, rgb(xp_color)),
        (
            unit.moves_left.map_or_else(
                || format!("–/{}", report.max_movement),
                |m| format!("{m}/{}", report.max_movement),
            ),
            TEXT,
        ),
    ];
    for (i, (value, color)) in stats.into_iter().enumerate() {
        let y = i as f32 * (line + 14.0) - 1.0;
        text(&value, 85.0, y + line, 58.0, line, tiny, color);
    }
    text(
        &format!("{}%", report.defense),
        143.0,
        3.0 * line + 27.0,
        31.0,
        line,
        tiny,
        IVORY,
    );

    let name_y = if frame.compact { 79.0 } else { 83.0 };
    let name_h = if frame.compact { 17.0 } else { 20.0 };
    let row = if frame.compact { 16.0 } else { 18.0 };
    if let Some(name) = &report.name {
        text(
            name,
            3.0,
            name_y,
            124.0,
            name_h,
            if frame.compact { 15 } else { 17 },
            WHITE,
        );
    }
    let flag = art.frame.side_flags.entry(unit.side).or_insert_with(|| {
        let palette = art.flag_colors.as_ref().map(|colors| (colors, unit.side));
        load_texture(&art.flag_dir.join("flag-icon.png"), palette)
    });
    paint(flag, rect(130.0, name_y, 25.0, 17.0));
    text(&unit.side.to_string(), 157.0, name_y, 15.0, name_h, small, TEXT);
    let mut y = name_y + name_h;
    text(
        movement.assets.unit_name(&unit.type_id).unwrap_or(&unit.type_id),
        3.0,
        y,
        137.0,
        row,
        small,
        TEXT,
    );
    text(&format!("lvl {}", report.level), 142.0, y, 30.0, row, small, TEXT);
    y += row;
    let alignment = match report.alignment {
        Alignment::Lawful => "Lawful",
        Alignment::Neutral => "Neutral",
        Alignment::Chaotic => "Chaotic",
        Alignment::Liminal => "Liminal",
    };
    text(
        &format!("{alignment} ({:+}%)", report.alignment_bonus),
        10.0,
        y,
        160.0,
        row,
        small,
        IVORY,
    );
    y += row;
    text(
        &movement.assets.unit_ability_names(&unit.type_id).join(", "),
        10.0,
        y,
        160.0,
        row,
        small,
        IVORY,
    );
    y += row;
    if let Some(race) = movement.assets.unit_race_name(&unit.type_id) {
        text(race, 3.0, y, 165.0, row, small, TEXT);
    }
    y += row;
    text(&report.traits.join(", "), 10.0, y, 160.0, row, small, IVORY);
    if report.weapons.is_empty() {
        return;
    }
    y += row + if frame.compact { 0.0 } else { 12.0 };
    text(
        if report.weapons.len() == 1 {
            "Attack"
        } else {
            "Attacks"
        },
        3.0,
        y,
        165.0,
        row,
        small,
        DETAILS,
    );
    y += row;
    for (i, weapon) in report.weapons.iter().enumerate() {
        if rect(0.0, y, 0.0, 21.0).bottom() > frame.end_turn.y - 20.0 * s {
            break;
        }
        let range = icons
            .get(&format!("{}_attack", weapon.range))
            .and_then(Option::as_ref);
        let kind = icons.get(&weapon.damage_type).and_then(Option::as_ref);
        let mut x = 3.0;
        for icon in [range, kind].into_iter().flatten() {
            let slot = rect(x, y + 3.0, 16.0, 16.0);
            draw_texture_ex(
                icon,
                slot.x,
                slot.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(slot.w, slot.h)),
                    ..Default::default()
                },
            );
            x += 16.0;
        }
        let name = movement
            .assets
            .weapon_name(&unit.type_id, i)
            .unwrap_or(&weapon.id);
        text(
            &format!("{}×{} {name}", weapon.damage, weapon.strikes),
            x + 3.0,
            y,
            167.0 - x,
            21.0,
            small,
            IVORY,
        );
        y += 21.0;
        if range.is_none() || kind.is_none() {
            text(
                &format!("{} – {}", weapon.range, weapon.damage_type),
                12.0,
                y,
                157.0,
                row,
                small,
                DETAILS,
            );
            y += row;
        }
        if !weapon.specials.is_empty() {
            text(&weapon.specials.join(", "), 12.0, y, 157.0, row, small, DETAILS);
            y += row;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::scenario_bundle::Bundle;

    #[test]
    fn hovering_an_enemy_shows_its_report_and_empty_ground_restores_selection() {
        let source = br#"
[units]
[unit_type]
id=Guard
hitpoints=20
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
side=1
type=Guard
[/side]
[side]
side=2
type=Guard
[/side]
[/scenario]"#;
        let bundle = Bundle::parse(source).unwrap();
        let view = bundle
            .start(1, |_| unreachable!())
            .unwrap()
            .observation_for(1)
            .unwrap();
        let selected = Some(Hex { x: 0, y: 0 });
        let hovered = Some(Hex { x: 2, y: 0 });
        assert_eq!(displayed_unit(&view, hovered, selected).unwrap().side, 2);
        assert_eq!(
            displayed_unit(&view, Some(Hex { x: 1, y: 0 }), selected)
                .unwrap()
                .side,
            1
        );
        assert!(displayed_unit(&view, None, None).is_none());
    }
}

use super::{ART_HEX_SIZE, load_recolored, load_texture};
use engine_lite::observation::{Observation, ObservedUnit};
use macroquad::prelude::*;
use rustnoth_gui::{assets::UnitBars, team_colors::TeamColors};
use std::{collections::HashMap, path::Path};

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub(super) enum Orb {
    Unmoved,
    Partial,
    Disengaged,
    Moved,
    Ally,
}

pub(super) fn orb_for(view: &Observation, unit: &ObservedUnit, actionable: bool) -> Option<Orb> {
    if unit.side != view.side {
        return view.allied_sides.contains(&unit.side).then_some(Orb::Ally);
    }
    Some(own_orb(unit.full_action_points == Some(true), actionable, unit.attacks_left == Some(0)))
}

fn own_orb(full: bool, actionable: bool, no_attacks: bool) -> Orb {
    if full { Orb::Unmoved }
    else if !actionable { Orb::Moved }
    else if no_attacks { Orb::Disengaged }
    else { Orb::Partial }
}

pub(super) struct StatusArt {
    orbs: HashMap<Orb, Texture2D>,
    crown: Option<Texture2D>,
}

impl StatusArt {
    pub(super) fn load(root: &Path, colors: Option<&[u8]>) -> Self {
        let dir = root.join("images/misc");
        let mut orbs = HashMap::new();
        if let Some(colors) = colors {
            for (orb, color) in [(Orb::Ally, "lightblue"), (Orb::Moved, "red"),
                (Orb::Partial, "brightorange"), (Orb::Unmoved, "brightgreen")] {
                let Ok(palette) = TeamColors::named_range(colors, "magenta", color) else { continue };
                if let Some(texture) = load_texture(&dir.join("orb.png"), Some((&palette, 1))) {
                    orbs.insert(orb, texture);
                }
                if orb == Orb::Partial
                    && let Ok(moved) = TeamColors::named_range(colors, "ellipse_red", "red")
                    && let Some(texture) = load_recolored(&dir.join("orb-two-color.png"), [(&moved, 1), (&palette, 1)])
                {
                    orbs.insert(Orb::Disengaged, texture);
                }
            }
        }
        Self { orbs, crown: load_texture(&dir.join("leader-crown.png"), None) }
    }

    pub(super) fn draw(&self, unit: &ObservedUnit, style: UnitBars, tile: Vec2, size: f32,
                       sprite_width: f32, orb: Option<Orb>, highlighted: bool, viewport: super::Viewport) {
        let zoom = size / ART_HEX_SIZE;
        // The original uses the sprite width on both axes unless the type supplies offsets.
        let offset = style.offset.map(|[x, y]| vec2(x as f32, y as f32))
            .unwrap_or_else(|| Vec2::splat(((size - sprite_width * zoom) / 2.0).trunc()));
        let origin = tile + offset;
        let orb = orb.and_then(|orb| self.orbs.get(&orb));
        let crown = self.crown.as_ref().filter(|_| unit.can_recruit);
        for texture in [orb, crown].into_iter().flatten() {
            if !super::visible_rect(viewport, origin, texture.size() * zoom) { continue; }
            draw_texture_ex(texture, origin.x, origin.y, WHITE, DrawTextureParams {
                dest_size: Some(vec2(texture.width(), texture.height()) * zoom),
                ..Default::default()
            });
        }

        let alpha = if highlighted { 255 } else { 204 };
        if unit.max_hitpoints > 0 {
            let ratio = unit.hitpoints as f32 / unit.max_hitpoints as f32;
            draw_bar(origin - vec2((5.0 * zoom).trunc(), 0.), zoom,
                unit.max_hitpoints as f32 * style.hp_scale, ratio, hp_color(ratio), alpha, viewport);
        }
        if unit.experience > 0 && unit.max_experience > 0 && let Some(major) = style.advancement {
            let ratio = unit.experience as f32 / unit.max_experience as f32;
            draw_bar(origin, zoom, unit.max_experience as f32 * style.xp_scale / style.level.max(1) as f32,
                ratio, xp_color(unit.max_experience.saturating_sub(unit.experience), major), alpha, viewport);
        }
    }
}

pub(super) fn hp_color(ratio: f32) -> [u8; 3] {
    if ratio > 1.0 { [100, 255, 100] }
    else if ratio == 1.0 { [33, 225, 0] }
    else if ratio >= 0.75 { [170, 255, 0] }
    else if ratio >= 0.50 { [255, 175, 0] }
    else if ratio >= 0.25 { [255, 155, 0] }
    else { [255, 0, 0] }
}

pub(super) fn xp_color(remaining: i32, major: bool) -> [u8; 3] {
    let thresholds = if major {
        [[255,255,255], [150,255,255], [0,205,205], [0,160,225]]
    } else {
        [[225,0,255], [169,30,255], [139,0,237], [170,0,255]]
    };
    thresholds[match remaining { ..=8 => 0, 9..=16 => 1, 17..=24 => 2, _ => 3 }]
}

fn draw_bar(origin: Vec2, zoom: f32, height: f32, ratio: f32, [r,g,b]: [u8; 3], alpha: u8, viewport: super::Viewport) {
    let x = origin.x + (19.0 * zoom).trunc();
    let dy = (13.0 * zoom).trunc();
    let y = origin.y + dy;
    let width = (4.0 * zoom).trunc();
    let height = (height.trunc() * zoom).trunc().min((ART_HEX_SIZE * zoom * 0.8 - dy).trunc());
    if width < 1.0 || height < 1.0 { return; }
    if !super::visible_rect(viewport, vec2(x, y) - Vec2::ONE, vec2(width, height) + Vec2::splat(2.)) { return; }
    let unfilled = (height * (1.0 - ratio.clamp(0.0, 1.0))).trunc();
    draw_rectangle(x, y, width, height, Color::from_rgba(0, 0, 0, 80));
    draw_rectangle(x, y + unfilled, width, height - unfilled, Color::from_rgba(r, g, b, alpha));
    draw_rectangle_lines(x, y, width, height, 1.0, Color::from_rgba(213, 213, 213, 200));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_distinguish_full_health_level_advancement_and_amla() {
        assert_eq!(hp_color(1.0), [33, 225, 0]);
        assert_eq!(hp_color(0.75), [170, 255, 0]);
        assert_eq!(hp_color(0.24), [255, 0, 0]);
        assert_eq!(xp_color(8, true), [255, 255, 255]);
        assert_eq!(xp_color(9, true), [150, 255, 255]);
        assert_eq!(xp_color(8, false), [225, 0, 255]);
    }

    #[test]
    fn orbs_reflect_an_untouched_spent_or_disengaged_unit() {
        assert_eq!(own_orb(true, false, false), Orb::Unmoved);
        assert_eq!(own_orb(false, false, true), Orb::Moved);
        assert_eq!(own_orb(false, true, true), Orb::Disengaged);
        assert_eq!(own_orb(false, true, false), Orb::Partial);
    }
}

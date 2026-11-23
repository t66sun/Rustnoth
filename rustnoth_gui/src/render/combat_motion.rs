//! Play the committed strikes once, using only the board as seen by the spectator.
use super::{BoardArt, BoardLayout, standing};
use engine_lite::{
    combat::ResolvedBlow,
    fixed_match::combat_display::{DisplayWeapon, VisibleCombat},
    hex::{Direction, Hex},
    wml::Node,
};
use macroquad::prelude::*;
use rustnoth_gui::{assets::AssetCatalog, session::Session, team_colors::TeamColors};
use std::{collections::HashMap, path::Path};

struct Animation {
    frames: Vec<standing::Frame>,
    missiles: Vec<standing::Frame>,
    missile_start: i64,
    start: i64,
    duration: u64,
    offset: Vec<(f32, f32, u64)>,
}

fn curve(expression: &str, duration: u64) -> Option<Vec<(f32, f32, u64)>> {
    let mut pieces = Vec::new();
    let mut defined = 0u64;
    let mut missing = 0u64;
    for item in expression.split(',') {
        let (values, ms) = match item.trim().split_once(':') {
            Some((values, ms)) => (values, Some(ms.parse::<u64>().ok()?)),
            None => (item.trim(), None),
        };
        let (a, b) = values.split_once('~').unwrap_or((values, values));
        let (from, to) = (a.parse::<f32>().ok()?, b.parse::<f32>().ok()?);
        if !from.is_finite() || !to.is_finite() {
            return None;
        }
        if let Some(n) = ms {
            defined = defined.saturating_add(n)
        } else {
            missing += 1
        }
        pieces.push((from, to, ms));
    }
    let share = duration.saturating_sub(defined) / missing.max(1);
    Some(
        pieces
            .into_iter()
            .map(|(a, b, t)| (a, b, t.unwrap_or(share).max(1)))
            .collect(),
    )
}

impl Animation {
    fn displacement(&self, mut time: u64) -> f32 {
        for &(a, b, ms) in &self.offset {
            if time < ms {
                return a + (b - a) * time as f32 / ms as f32;
            }
            time -= ms;
        }
        self.offset.last().map_or(0.0, |&(_, b, _)| b)
    }
    fn fallback(melee: bool) -> Self {
        Self {
            frames: vec![],
            missiles: vec![],
            missile_start: 0,
            start: -150,
            duration: 300,
            offset: if melee {
                vec![(0.0, 0.6, 150), (0.6, 0.0, 150)]
            } else {
                vec![]
            },
        }
    }
}

struct Conditions<'a> {
    direction: Direction,
    outcome: &'a str,
    weapon: &'a DisplayWeapon,
    offense: bool,
}

impl Conditions<'_> {
    fn score(&self, node: &Node) -> Option<usize> {
        let contains = |list: &str, value: &str| list.split(',').any(|s| s.trim() == value);
        let mut score = 0;
        if let Some(dir) = node.attribute("direction") {
            if !dir
                .split(',')
                .any(|s| Direction::from_wml(s) == Some(self.direction))
            {
                return None;
            }
            score += 1;
        }
        if let Some(hits) = node.attribute("hits") {
            if !contains(hits, self.outcome) {
                return None;
            }
            score += 1;
        }
        if [
            "terrain_type",
            "value",
            "value_second",
            "swing",
            "frequency",
        ]
        .iter()
        .any(|k| node.attribute(k).is_some())
        {
            return None;
        }
        for (tag, child) in &node.children {
            if tag.starts_with("filter") {
                if !self.offense || tag != "filter_attack" || !child.children.is_empty() {
                    return None;
                }
                for (key, value) in &child.attributes {
                    let field = match key.as_str() {
                        "name" => &self.weapon.id,
                        "range" => &self.weapon.range,
                        _ => return None,
                    };
                    if !contains(&value.text, field) {
                        return None;
                    }
                }
                score += 1;
            }
        }
        Some(score)
    }

    fn select(&self, nodes: &[(String, Node)]) -> Animation {
        let melee = self.offense && self.weapon.range == "melee";
        nodes
            .iter()
            .filter(|(tag, _)| {
                tag == if self.offense {
                    "attack_anim"
                } else {
                    "defend"
                }
            })
            .filter_map(|(_, node)| self.read(node))
            .max_by_key(|(score, _)| *score)
            .map(|(_, anim)| anim)
            .unwrap_or_else(|| Animation::fallback(melee))
    }

    fn read(&self, node: &Node) -> Option<(usize, Animation)> {
        let mut score = self.score(node)?;
        let mut frames = Vec::new();
        let mut offset = node.attribute("offset");
        let mut missiles = Vec::new();
        let flip = !matches!(node.attribute("auto_hflip"), Some("no" | "false" | "0"));
        let mut matched = false;
        for (tag, child) in &node.children {
            if tag == "frame" {
                standing::append_sprite_frame(&mut frames, child, flip, true)?;
            } else if tag == "missile_frame" && self.offense {
                let mut frame = child.clone();
                if !matches!(self.direction, Direction::North | Direction::South)
                    && let Some(diagonal) = frame.attributes.get("image_diagonal").cloned()
                {
                    frame.attributes.insert("image".into(), diagonal);
                }
                // Halos and procedural particles have no main projectile sprite.
                if frame.attribute("image").is_some() {
                    standing::append_sprite_frame(&mut missiles, &frame, true, true)?;
                }
            } else if tag == "if" || tag == "else" {
                if tag == "if" {
                    matched = false
                }
                if matched {
                    continue;
                }
                let Some(s) = self.score(child) else { continue };
                matched = true;
                score += s;
                if let Some(value) = child.attribute("offset") {
                    offset = Some(value);
                }
                for (tag, frame) in &child.children {
                    if tag == "frame" {
                        standing::append_sprite_frame(&mut frames, frame, flip, true)?;
                    } else if tag == "if" || tag == "else" {
                        return None;
                    }
                }
            }
        }
        let duration = frames.last()?.end_ms;
        let start = node
            .attribute("start_time")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let default_offset = if self.offense && self.weapon.range == "melee" {
            "0~0.6,0.6~0"
        } else {
            "0"
        };
        let missile_start = node
            .attribute("missile_start_time")
            .and_then(|v| v.parse().ok())
            .unwrap_or(start);
        Some((
            score,
            Animation {
                frames,
                missiles,
                missile_start,
                start,
                duration,
                offset: curve(offset.unwrap_or(default_offset), duration)?,
            },
        ))
    }
}

fn sprite(
    assets: &AssetCatalog,
    expression: &str,
    palette: Option<(&TeamColors, usize)>,
) -> Option<Texture2D> {
    let mut parts = expression.split('~');
    let base = assets.resolve_image(Path::new(parts.next()?))?;
    if !expression.contains('~') {
        return super::load_texture(&base, palette);
    }
    let mut pixels = ::image::open(base).ok()?.to_rgba8();
    for modifier in parts {
        let mut args = modifier
            .strip_prefix("BLIT(")?
            .strip_suffix(')')?
            .split(',')
            .map(str::trim);
        let layer = ::image::open(assets.resolve_image(Path::new(args.next()?))?)
            .ok()?
            .to_rgba8();
        let x = args.next().unwrap_or("0").parse().ok()?;
        let y = args.next().unwrap_or("0").parse().ok()?;
        ::image::imageops::overlay(&mut pixels, &layer, x, y);
    }
    if let Some((colors, side)) = palette {
        colors.recolor(side, pixels.as_mut());
    }
    let tex = Texture2D::from_rgba8(
        pixels.width().try_into().ok()?,
        pixels.height().try_into().ok()?,
        pixels.as_raw(),
    );
    tex.set_filter(FilterMode::Linear);
    Some(tex)
}

struct Actor {
    at: Hex,
    offset: f32,
    frame: Option<(Texture2D, bool)>,
}

pub(super) struct Pose {
    actors: [Actor; 2],
    damage: Option<(u32, f32)>,
    missile: Option<(Texture2D, f32, Direction)>,
}

impl Pose {
    pub(super) fn sprite(
        &self,
        at: Hex,
        center: impl Fn(Hex, f32) -> Vec2,
    ) -> Option<(Vec2, Option<(&Texture2D, bool)>)> {
        let i = self.actors.iter().position(|u| u.at == at)?;
        let actor = &self.actors[i];
        Some((
            center(self.actors[1 - i].at, actor.offset),
            actor.frame.as_ref().map(|(t, b)| (t, *b)),
        ))
    }

    pub(super) fn draw_impact(&self, layout: &BoardLayout, font: Option<&Font>, unit_origin: impl Fn(Hex) -> Vec2) {
        if let Some((tex, phase, dir)) = &self.missile {
            let origin =
                |h| unit_origin(h) + Vec2::splat(layout.tile_size() * 0.5);
            let at = origin(self.actors[0].at).lerp(origin(self.actors[1].at), *phase);
            let size = tex.size() * (layout.tile_size() / super::ART_HEX_SIZE);
            draw_texture_ex(
                tex,
                at.x - size.x * 0.5,
                at.y - size.y * 0.5,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(size),
                    flip_x: standing::west(*dir),
                    flip_y: matches!(
                        dir,
                        Direction::South | Direction::SouthEast | Direction::SouthWest
                    ),
                    ..Default::default()
                },
            );
        }
        if let Some((loss, progress)) = self.damage {
            let p = unit_origin(self.actors[1].at);
            let text = loss.to_string();
            let size = (layout.tile_size() * 0.31).clamp(12.0, 30.0) as u16;
            let x = p.x + (layout.tile_size() - measure_text(&text, font, size, 1.0).width) * 0.5;
            let y = p.y + layout.tile_size() * (0.39 - progress * 0.35);
            crate::dialog::text(font, &text, x + 1.0, y + 1.0, size, BLACK);
            crate::dialog::text(font, &text, x, y, size, Color::from_rgba(255, 65, 65, 255));
        }
    }
}

pub(crate) async fn show(
    mut fight: VisibleCombat,
    session: &Session<'_>,
    layout: &BoardLayout,
    art: &mut BoardArt,
    assets: &AssetCatalog,
    mut draw_controls: impl FnMut(&mut BoardArt),
) {
    let mut textures = HashMap::<(usize, String), Option<Texture2D>>::new();
    let [Some(attacker), Some(defender)] = fight
        .participants
        .map(|hex| fight.opening.units.iter().position(|u| u.position == hex))
    else {
        return;
    };
    let indices = [attacker, defender];
    'combat: for blow in fight.blows {
        let i = usize::from(blow.retaliation);
        let pos = [fight.participants[i], fight.participants[1 - i]];
        let Some(weapon) = &fight.weapons[i] else {
            continue;
        };
        let directions = [pos[0].direction_to(pos[1]), pos[1].direction_to(pos[0])];
        let [Some(attacking), Some(defending)] = directions else {
            continue;
        };
        let facings = [attacking, defending];
        let outcome = if !blow.hit {
            "miss"
        } else if blow.remaining_hp[1 - i] == 0 {
            "kill"
        } else {
            "hit"
        };
        let roles = [indices[i], indices[1 - i]];
        let plans = [0, 1].map(|j| {
            let unit = &fight.opening.units[roles[j]];
            let anim = Conditions {
                direction: facings[j],
                outcome,
                weapon,
                offense: j == 0,
            }
            .select(assets.combat_animations(&unit.type_id));
            for frame in anim.frames.iter().chain(&anim.missiles) {
                textures
                    .entry((unit.side, frame.path.clone()))
                    .or_insert_with(|| {
                        sprite(
                            assets,
                            &frame.path,
                            art.team_colors.as_ref().map(|p| (p, unit.side)),
                        )
                    });
            }
            anim
        });
        let start = plans
            .iter()
            .map(|a| {
                if a.missiles.is_empty() {
                    a.start
                } else {
                    a.start.min(a.missile_start)
                }
            })
            .min()
            .unwrap_or(-150)
            .min(0);
        let end = plans
            .iter()
            .map(|a| {
                let end = a.start.saturating_add(a.duration as i64);
                a.missiles.last().map_or(end, |f| {
                    end.max(a.missile_start.saturating_add(f.end_ms as i64))
                })
            })
            .max()
            .unwrap_or(150)
            .max(120);
        let since = get_time();
        let rate = f64::from(art.animation_rate());
        loop {
            let time = start.saturating_add(((get_time() - since) * 1000.0 * rate) as i64);
            if is_key_pressed(KeyCode::Escape) {
                break 'combat;
            }
            if time >= end {
                break;
            }
            let actors = [0, 1].map(|j| {
                let unit = &mut fight.opening.units[roles[j]];
                unit.facing = Some(facings[j]);
                if time >= 0 {
                    unit.hitpoints = blow.remaining_hp[if j == 0 { i } else { 1 - i }] as i32;
                }
                let plan = &plans[j];
                let phase = time.saturating_sub(plan.start).max(0) as u64;
                let frame = plan
                    .frames
                    .iter()
                    .find(|f| time >= plan.start && phase < f.end_ms)
                    .and_then(|f| {
                        Some((
                            textures.get(&(unit.side, f.path.clone()))?.clone()?,
                            f.auto_hflip && standing::west(facings[j]),
                        ))
                    });
                Actor {
                    at: pos[j],
                    offset: plan.displacement(phase),
                    frame,
                }
            });
            let missile = &plans[0];
            let elapsed = time.saturating_sub(missile.missile_start);
            let projectile = u64::try_from(elapsed).ok().and_then(|ms| {
                let frame = missile.missiles.iter().find(|f| ms < f.end_ms)?;
                let team = fight.opening.units[roles[0]].side;
                let texture = textures.get(&(team, frame.path.clone()))?.clone()?;
                Some((
                    texture,
                    ms as f32 / missile.missiles.last()?.end_ms as f32,
                    attacking,
                ))
            });
            art.combat_pose = Some(Pose {
                actors,
                damage: damage_label(&blow, time, end),
                missile: projectile,
            });
            super::draw(
                session.state(),
                Some(&fight.opening),
                Some(layout),
                art,
                super::MovementView {
                    session,
                    global_time: session.global_time_of_day(),
                    route: None,
                    selected: None,
                    actions: &[],
                    message: None,
                    assets,
                    menu: None,
                },
                &mut draw_controls,
            );
            next_frame().await;
        }
        for (role, hp) in indices.iter().zip(blow.remaining_hp) {
            fight.opening.units[*role].hitpoints = hp as i32;
        }
    }
    art.combat_pose = None;
}

fn damage_label(blow: &ResolvedBlow, time: i64, end: i64) -> Option<(u32, f32)> {
    (time >= 0 && blow.hit && blow.lost_hp > 0).then(|| (blow.lost_hp, time as f32 / end as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directional_weapon_art_and_offsets_follow_the_actual_hit() {
        let doc=engine_lite::wml::parse("[attack_anim]\nstart_time=-100\ndirection=sw,se\noffset=0~0.5:100,0.5~0:100\n[filter_attack]\nname=spear\n[/filter_attack]\n[frame]\nimage=units/spear-[1,2].png:100\n[/frame]\n[/attack_anim]").unwrap();
        let weapon = DisplayWeapon {
            id: "spear".into(),
            range: "melee".into(),
        };
        let ctx = Conditions {
            direction: Direction::SouthWest,
            outcome: "hit",
            weapon: &weapon,
            offense: true,
        };
        let anim = ctx.select(&doc.children);
        assert_eq!(anim.frames[1].path, "units/spear-2.png");
        assert_eq!(
            (anim.start, anim.duration, anim.displacement(100)),
            (-100, 200, 0.5)
        );
        let ctx = Conditions {
            direction: Direction::North,
            ..ctx
        };
        assert!(ctx.select(&doc.children).frames.is_empty());
        let blow = ResolvedBlow {
            retaliation: false,
            hit: true,
            lost_hp: 4,
            remaining_hp: [20, 6],
        };
        assert_eq!(damage_label(&blow, -1, 150), None);
        assert_eq!(damage_label(&blow, 0, 150), Some((4, 0.0)));
    }
}

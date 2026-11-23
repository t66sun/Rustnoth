//! Weapon selection from the stock unit_attack window.
use super::{BoardArt, atlas::AtlasSprite, get_texture};
use crate::{dialog, menu::Menu};
use engine_lite::{
    board::AttackChoice,
    fixed_match::{
        Action,
        attack_preview::{AttackPreview, StrikePreview},
    },
    observation::{Observation, ObservedUnit},
};
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;
use std::{fs, path::Path};

const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
const LIGHT: Color = Color::from_rgba(221, 221, 221, 255);
const EDGE: Color = Color::from_rgba(114, 79, 46, 255);
const PANEL_WIDTH: f32 = 710.0;
const ROW_HEIGHT: f32 = 66.0;

struct UnitPane {
    title: String,
    hitpoints: i32,
    xp: String,
    sprite: Option<AtlasSprite>,
    portrait: Option<Texture2D>,
}

impl UnitPane {
    fn new(unit: &ObservedUnit, assets: &AssetCatalog, art: &mut BoardArt) -> Self {
        let portrait = assets
            .unit_portrait(&unit.type_id)
            .and_then(|path| get_texture(&mut art.paths, path));
        Self {
            title: assets
                .unit_name(&unit.type_id)
                .unwrap_or(&unit.type_id)
                .into(),
            hitpoints: unit.hitpoints,
            xp: format!("XP   {}/{}", unit.experience, unit.max_experience),
            sprite: art
                .units
                .get(&unit.type_id)
                .and_then(|units| units.get(&unit.side))
                .cloned()
                .flatten(),
            portrait,
        }
    }
}

struct WeaponPane {
    title: String,
    stats: String,
    image: Option<Texture2D>,
}

impl WeaponPane {
    fn new(unit: &str, strike: &StrikePreview, assets: &AssetCatalog, art: &mut BoardArt) -> Self {
        Self {
            title: assets
                .weapon_name(unit, strike.weapon)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Weapon {}", strike.weapon + 1)),
            stats: format!(
                "{} × {}   ({}%)",
                strike.damage, strike.strikes, strike.hit_chance
            ),
            image: assets
                .weapon_icon(unit, strike.weapon)
                .filter(|p| p.is_file())
                .and_then(|p| get_texture(&mut art.paths, p)),
        }
    }
}

struct WeaponRow {
    choice: AttackChoice,
    offense: WeaponPane,
    response: Option<WeaponPane>,
    range: String,
    damage_type: String,
}

pub(crate) struct AttackDialog {
    units: [UnitPane; 2],
    rows: Vec<WeaponRow>,
    font: Option<Font>,
    panel: Option<Texture2D>,
    selection: Option<Texture2D>,
    button: [Option<Texture2D>; 3],
}

fn image(texture: Option<&Texture2D>, rect: Rect, contain: bool, mirror: bool) {
    if let Some(texture) = texture {
        let size = if contain {
            texture.size() * (rect.w / texture.width()).min(rect.h / texture.height())
        } else {
            vec2(rect.w, rect.h)
        };
        let pos = rect.center() - size * 0.5;
        draw_texture_ex(
            texture,
            pos.x,
            pos.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(size),
                flip_x: mirror,
                ..Default::default()
            },
        );
    }
}

impl AttackDialog {
    pub(crate) fn new(
        view: &Observation,
        origin: engine_lite::hex::Hex,
        previews: Vec<AttackPreview>,
        assets: &AssetCatalog,
        art: &mut BoardArt,
        root: &Path,
    ) -> Option<Self> {
        let choice = previews.first()?.choice;
        let unit_at = |hex| view.units.iter().find(|unit| unit.position == hex);
        let (attacker, defender) = (unit_at(origin)?, unit_at(choice.to)?);
        let units = [
            UnitPane::new(attacker, assets, art),
            UnitPane::new(defender, assets, art),
        ];
        let rows = previews
            .into_iter()
            .map(|preview| WeaponRow {
                choice: preview.choice,
                offense: WeaponPane::new(&attacker.type_id, &preview.offense, assets, art),
                response: preview
                    .response
                    .as_ref()
                    .map(|strike| WeaponPane::new(&defender.type_id, strike, assets, art)),
                range: preview.offense.range,
                damage_type: preview.offense.damage_type,
            })
            .collect();
        let mut texture = |name: &str| get_texture(&mut art.paths, root.join("images").join(name));
        Some(Self {
            units,
            rows,
            font: fs::read(root.join("fonts/Lato-Regular.ttf"))
                .ok()
                .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok()),
            panel: texture("dialogs/opaque-background.png"),
            selection: texture("dialogs/selection-background-fancy.png"),
            button: [
                "background.png",
                "background-active.png",
                "background-pressed.png",
            ]
            .map(|name| texture(&format!("buttons/button_normal/{name}"))),
        })
    }

    fn label(&self, value: &str, rect: Rect, size: u16, color: Color, align: f32) {
        let font = self.font.as_ref();
        let text = dialog::fit(font, value.into(), rect.w, size);
        let dim = measure_text(&text, font, size, 1.0);
        let x = rect.x + (rect.w - dim.width) * align;
        let y = rect.center().y + dim.offset_y - dim.height * 0.5;
        dialog::text(font, &text, x.round(), y.round(), size, color);
    }

    fn button(&self, area: Rect, caption: &str, pointer: Vec2) -> bool {
        let over = area.contains(pointer);
        let state = if over && is_mouse_button_down(MouseButton::Left) {
            2
        } else {
            usize::from(over)
        };
        draw_rectangle(
            area.x,
            area.y,
            area.w,
            area.h,
            Color::from_rgba(15, 42, 57, 255),
        );
        image(self.button[state].as_ref(), area, false, false);
        draw_rectangle_lines(area.x, area.y, area.w, area.h, 2.0, EDGE);
        self.label(caption, area, 16, GOLD, 0.5);
        over && is_mouse_button_pressed(MouseButton::Left)
    }

    fn unit(&self, index: usize, area: Rect, profile: bool) {
        let pane = &self.units[index];
        let right = index == 1;
        let left = if right { area.right() - 144.0 } else { area.x };
        let portrait_cell = Rect::new(left, area.y, 144.0, 144.0);
        if profile && let Some(portrait) = pane.portrait.as_ref() {
            image(Some(portrait), portrait_cell, true, right);
        } else if let Some(sprite) = pane.sprite.as_ref() {
            let original = sprite.size();
            let size = original * (portrait_cell.w / original.x).min(portrait_cell.h / original.y);
            let pos = portrait_cell.center() - size * 0.5;
            sprite.draw(Rect::new(pos.x, pos.y, size.x, size.y), WHITE, right);
        }
        let x = if right { area.x } else { left + 148.0 };
        let mut rect = Rect::new(x, area.y + 12.0, area.w - 151.0, 25.0);
        self.label(&pane.title, rect, 20, GOLD, 0.0);
        rect.y += 30.0;
        self.label(&format!("HP   {}", pane.hitpoints), rect, 16, LIGHT, 0.0);
        rect.y += 23.0;
        self.label(&pane.xp, rect, 16, LIGHT, 0.0);
    }

    fn row(&self, index: usize, bounds: Rect, selected: bool, hover: bool) {
        if selected || hover {
            draw_rectangle(
                bounds.x,
                bounds.y,
                bounds.w,
                bounds.h,
                Color::from_rgba(45, 62, 69, 255),
            );
            image(self.selection.as_ref(), bounds, false, false);
            draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 1.0, EDGE);
        }
        let row = &self.rows[index];
        for (side, weapon) in [Some(&row.offense), row.response.as_ref()]
            .into_iter()
            .enumerate()
        {
            let right = side == 1;
            let x = if right {
                bounds.right() - 60.0
            } else {
                bounds.x + 5.0
            };
            let text_x = if right {
                bounds.right() - 248.0
            } else {
                bounds.x + 66.0
            };
            let text = Rect::new(text_x, bounds.y + 6.0, 182.0, 25.0);
            let align = if right { 1.0 } else { 0.0 };
            if let Some(weapon) = weapon {
                image(
                    weapon.image.as_ref(),
                    Rect::new(x, bounds.y + 5.0, 54.0, 54.0),
                    true,
                    false,
                );
                self.label(&weapon.title, text, 16, LIGHT, align);
                self.label(
                    &weapon.stats,
                    Rect {
                        y: text.y + 25.0,
                        ..text
                    },
                    15,
                    LIGHT,
                    align,
                );
            } else {
                self.label("—", text, 16, LIGHT, align);
            }
        }
        let center = Rect::new(bounds.center().x - 76.0, bounds.y + 7.0, 152.0, 24.0);
        self.label(&row.range, center, 16, GOLD, 0.5);
        self.label(
            &row.damage_type,
            Rect {
                y: center.y + 24.0,
                ..center
            },
            14,
            LIGHT,
            0.5,
        );
    }

    pub(crate) async fn choose(
        &self,
        menu: &mut Menu,
        assets: &AssetCatalog,
        art: &BoardArt,
    ) -> Option<Action> {
        let count = self.rows.len();
        let visible = count.min(5);
        let height = 252.0 + visible as f32 * ROW_HEIGHT;
        let mut cursor = 0usize;
        let mut first = 0usize;
        let mut profiles = [false; 2];
        let mut opening = true;
        let mut last_click: Option<(usize, f64)> = None;
        loop {
            art.surface.present();
            let scale = (screen_width() / (PANEL_WIDTH + 12.0))
                .min(screen_height() / (height + 12.0))
                .clamp(0.01, 1.0);
            let (w, h) = (screen_width() / scale, screen_height() / scale);
            set_camera(&dialog::setup_camera(w, h));
            let p = Vec2::from(mouse_position()) / scale;
            let bounds = Rect::new(
                (w - PANEL_WIDTH) * 0.5,
                (h - height) * 0.5,
                PANEL_WIDTH,
                height,
            );
            let clicked = !opening && is_mouse_button_pressed(MouseButton::Left);
            draw_rectangle(
                bounds.x,
                bounds.y,
                bounds.w,
                bounds.h,
                Color::from_rgba(26, 28, 30, 255),
            );
            image(self.panel.as_ref(), bounds, false, false);
            draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 3.0, EDGE);
            self.label(
                "Attack Enemy",
                Rect::new(bounds.x + 12.0, bounds.y + 3.0, 350.0, 42.0),
                28,
                GOLD,
                0.0,
            );
            let split = bounds.center().x;
            draw_line(split, bounds.y + 46.0, split, bounds.y + 190.0, 1.0, EDGE);
            for (i, profile) in profiles.iter_mut().enumerate() {
                let pane = Rect::new(
                    bounds.x + 10.0 + i as f32 * 347.0,
                    bounds.y + 45.0,
                    342.0,
                    144.0,
                );
                self.unit(i, pane, *profile);
                let x = if i == 0 { pane.x + 148.0 } else { pane.x };
                if self.units[i].portrait.is_some()
                    && self.button(Rect::new(x, pane.y + 101.0, 86.0, 29.0), "Profile", p)
                    && !opening
                {
                    *profile = !*profile;
                }
            }
            let list = Rect::new(
                bounds.x + 9.0,
                bounds.y + 194.0,
                PANEL_WIDTH - 18.0,
                visible as f32 * ROW_HEIGHT,
            );
            let wheel = if list.contains(p) {
                mouse_wheel().1
            } else {
                0.0
            };
            if !opening {
                if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::Tab) || wheel < 0.0 {
                    cursor = (cursor + 1) % count;
                }
                if is_key_pressed(KeyCode::Up) || wheel > 0.0 {
                    cursor = (cursor + count - 1) % count;
                }
            }
            first = first
                .clamp((cursor + 1).saturating_sub(visible), cursor)
                .min(count - visible);
            let mut confirmed = false;
            for (slot, index) in (first..first + visible).enumerate() {
                let row = Rect::new(
                    list.x,
                    list.y + slot as f32 * ROW_HEIGHT,
                    list.w,
                    ROW_HEIGHT,
                );
                let hover = row.contains(p);
                if clicked && hover {
                    cursor = index;
                    confirmed =
                        last_click.is_some_and(|(old, t)| old == index && get_time() - t <= 0.35);
                    last_click = Some((index, get_time()));
                }
                self.row(index, row, cursor == index, hover);
            }
            let y = bounds.bottom() - 42.0;
            if menu.advancement_button(list).is_some() {
                let name = menu
                    .preferred_advancement()
                    .map(|id| assets.unit_name(id).unwrap_or(id))
                    .unwrap_or("Default");
                let toggle = self.button(
                    Rect::new(list.x, y, 222.0, 30.0),
                    &format!("Advance to   {name}  [P]"),
                    p,
                );
                if !opening && (toggle || is_key_pressed(KeyCode::P)) {
                    menu.cycle_advancement();
                }
            }
            let attack = self.button(
                Rect::new(bounds.right() - 218.0, y, 98.0, 30.0),
                "Attack",
                p,
            );
            let cancel = self.button(
                Rect::new(bounds.right() - 112.0, y, 98.0, 30.0),
                "Cancel",
                p,
            );
            set_default_camera();
            if !opening
                && (cancel
                    || is_key_pressed(KeyCode::Escape)
                    || is_mouse_button_pressed(MouseButton::Right))
            {
                return None;
            }
            if !opening && (confirmed || attack || is_key_pressed(KeyCode::Enter)) {
                return Some(Action::Attack(self.rows[cursor].choice));
            }
            opening = false;
            next_frame().await;
        }
    }
}

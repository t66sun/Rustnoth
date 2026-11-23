//! Geometry and bitmaps of the default Wesnoth 1.18 battlefield theme.
use super::*;
mod unit_report;
mod end_button;
mod clock;

pub(super) struct BattlefieldFrame {
    pub map: Viewport,
    pub bar: Rect,
    pub sidebar: Rect,
    pub minimap_frame: Rect,
    pub minimap: Viewport,
    pub end_turn: Rect,
    pub compact: bool,
    pub scale: f32,
}

impl BattlefieldFrame {
    pub fn new(width: f32, height: f32) -> Self {
        let scale = (width / 800.0).min(height / 600.0).clamp(0.0, 1.0);
        let compact = width < 1024.0 || height < 768.0;
        let sidebar_x = width - 182.0 * scale;
        let bar_height = 27.0 * scale;
        let (mini_height, inset_x, inset_y, mini_width, inner_height) = if compact {
            (170.0, 43.0, 4.0, 130.0, 158.0)
        } else {
            (255.0, 13.0, 5.0, 160.0, 212.0)
        };
        let (right, bottom, button_w, button_h) = if compact {
            (140.0, 24.0, 80.0, 22.0)
        } else {
            (173.0, 50.0, 168.0, 44.0)
        };
        Self {
            map: Viewport { x: 0.0, y: bar_height, width: sidebar_x, height: height - bar_height },
            bar: Rect::new(0.0, 0.0, sidebar_x, bar_height),
            sidebar: Rect::new(sidebar_x, 0.0, width - sidebar_x, height),
            minimap_frame: Rect::new(sidebar_x, 6.0 * scale, 181.0 * scale, mini_height * scale),
            minimap: Viewport {
                x: sidebar_x + inset_x * scale,
                y: (6.0 + inset_y) * scale,
                width: mini_width * scale,
                height: inner_height * scale,
            },
            end_turn: Rect::new(width - right * scale, height - bottom * scale, button_w * scale, button_h * scale),
            compact,
            scale,
        }
    }

    pub fn menu(&self) -> Rect {
        let margin = 8.0 * self.scale;
        let top = self.minimap_frame.bottom() + 30.0;
        Rect::new(self.sidebar.x + margin, top, self.sidebar.w - 2.0 * margin,
            (self.end_turn.y - 24.0 - top).max(0.0))
    }
}

impl BoardArt {
    pub(crate) fn minimap_input(&mut self, point: Vec2, camera: &mut crate::camera::BoardCamera) -> std::io::Result<bool> {
        let frame = BattlefieldFrame::new(screen_width(), screen_height());
        self.frame.minimap_buttons.input(frame.minimap_frame, point, camera, &mut self.minimap)
    }
}

type Texture = Option<Texture2D>;

pub(super) struct FrameArt {
    bar: Texture,
    sidebar: Texture,
    minimap: [Texture; 2],
    minimap_buttons: crate::minimap_buttons::Buttons,
    end_button: end_button::Art,
    reports: [Texture; 5],
    clock: clock::Clock,
    side_flags: BTreeMap<usize, Texture>,
    unit_report: unit_report::Art,
    status: StatusBox,
}

impl FrameArt {
    pub fn clear_unit_report(&mut self) { self.unit_report.cached = None; }

    pub fn load(root: &Path, locale: &crate::title_screen::locale::Locale) -> Self {
        let themes = root.join("data/core/images/themes");
        let theme = |name: &str| load_texture(&themes.join(name), None);
        Self {
            bar: theme("classic/menubar.png"),
            sidebar: theme("classic/sidebar.png"),
            minimap: [theme("classic/minimap.png"), theme("classic/minimap-800.png")],
            minimap_buttons: crate::minimap_buttons::Buttons::load(root),
            end_button: end_button::Art::load(root),
            clock: clock::Clock::new(root),
            side_flags: BTreeMap::new(),
            unit_report: unit_report::Art::new(root, locale),
            reports: ["gold.png", "villages.png", "units.png", "upkeep.png", "income.png"].map(theme),
            status: StatusBox::load(root),
        }
    }
}

pub(crate) struct StatusBox([Texture; 9]);

impl StatusBox {
    pub(crate) fn load(root: &Path) -> Self {
        let dir = root.join("data/core/images/themes/status_box");
        Self([
                "border-topleft", "border-top", "border-topright", "border-left", "background",
                "border-right", "border-botleft", "border-bottom", "border-botright",
            ].map(|name| load_texture(&dir.join(format!("status_box-{name}.png")), None)))
    }

    pub(crate) fn draw(&self, bounds: Rect, scale: f32) {
        let border = 3.0 * scale;
        let xs = [bounds.x, bounds.x + border, bounds.right() - border, bounds.right()];
        let ys = [bounds.y, bounds.y + border, bounds.bottom() - border, bounds.bottom()];
        for (row, ys) in ys.windows(2).enumerate() {
            for (col, xs) in xs.windows(2).enumerate() {
                paint(&self.0[row * 3 + col], Rect::new(xs[0], ys[0], xs[1] - xs[0], ys[1] - ys[0]));
            }
        }
    }
}

fn paint(texture: &Texture, bounds: Rect) {
    if let Some(texture) = texture {
        draw_texture_ex(texture, bounds.x, bounds.y, WHITE, DrawTextureParams {
            dest_size: Some(vec2(bounds.w, bounds.h)), ..Default::default()
        });
    }
}

fn fill(bounds: Rect, color: Color) {
    draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, color);
}

fn text_in(font: &Option<Font>, text: &str, bounds: Rect, size: u16, color: Color, centered: bool) {
    if text.is_empty() || bounds.w <= 0.0 || bounds.h <= 0.0 { return; }
    let size = size.min(bounds.h.ceil() as u16).max(1);
    let measured = measure_text(text, font.as_ref(), size, 1.0);
    let scale = (bounds.w / measured.width.max(1.0)).min(1.0);
    let x = if centered { bounds.x + (bounds.w - measured.width * scale) * 0.5 } else { bounds.x };
    let y = bounds.center().y + (measured.offset_y - measured.height * 0.5) * scale;
    draw_text_ex(text, x, y, TextParams { font: font.as_ref(), font_size: size, font_scale: scale, color, ..Default::default() });
}

pub fn turn_button(art: &mut BoardArt, bounds: Rect, highlighted: bool, enabled: bool, title: &str) {
    art.frame.end_button.draw(bounds, highlighted, enabled, title, &art.dialogue_font);
}

pub fn draw(
    frame: &BattlefieldFrame, state: MatchState, observation: Option<&Observation>,
    board: Option<&BoardLayout>, art: &mut BoardArt, movement: &MovementView<'_>,
) {
    fill(frame.bar, PANEL);
    fill(frame.sidebar, PANEL);
    // Classic theme panels repeat the installed bitmap and clip the last tile.
    for (texture, bounds) in [(&art.frame.bar, frame.bar), (&art.frame.sidebar, frame.sidebar)] {
        if let Some(texture) = texture {
            crate::title_screen::tiled(bounds, texture);
        }
    }
    let s = frame.scale;
    if s <= 0.0 { return; }
    draw_reports(frame, state, observation, movement.session.economy(), art);
    art.frame.clock.draw(frame, art.dialogue_font.as_ref(), #[cfg(windows)] art.dialogue_text.renderer());
    paint(&art.frame.minimap[usize::from(frame.compact)], frame.minimap_frame);
    if let Some(view) = observation {
        art.minimap.draw(view, frame.minimap, frame.map, board, art.team_colors.as_ref(), movement);
        let zoom = board.map_or(72., |layout| layout.tile_size());
        art.frame.minimap_buttons.draw(frame.minimap_frame, Some(Vec2::from(mouse_position())), zoom, art.minimap.layers);
    }

    if movement.menu.is_none() {
        let left = frame.sidebar.x + 7.0 * s;
        let top = frame.minimap_frame.bottom();
        let rect = |x, y, w, h| Rect::new(left + x * s, top + y * s, w * s, h * s);
        let (x, y) = mouse_position();
        let hovered = art.briefing_highlight.or_else(|| {
            board.filter(|_| frame.map.contains([x, y])).and_then(|b| b.hex_at([x, y]))
        });
        let position = hovered.or(movement.selected);
        art.frame.status.draw(rect(0.0, 0.0, 171.0, 21.0), s);
        art.frame.status.draw(rect(0.0, 19.0, 171.0, 21.0), s);
        if let Some(hex) = position {
            text_in(&art.font, &format!("{}, {}", hex.x + 1, hex.y + 1), rect(5.0, 2.0, 161.0, 17.0), 14, TEXT, false);
            if let Some(tile) = observation.and_then(|v| v.tiles.iter().find(|t| t.position == hex)) {
                let terrain = movement.assets.terrain_name(&tile.terrain).unwrap_or(&tile.terrain);
                text_in(&art.font, terrain, rect(5.0, 21.0, 161.0, 17.0), 14, TEXT, false);
            }
        }
        art.frame.status.draw(rect(0.0, 43.0, 128.0, 42.0), s);
        if let Some(period) = movement.global_time {
            let (name, image) = art.periods.get(period.id.as_str())
                .map_or((period.id.as_str(), None), |(name, image)| (*name, image.as_ref()));
            let bounds = rect(3.0, 46.0, 122.0, 36.0);
            if let Some(image) = image {
                draw_texture_ex(image, bounds.x, bounds.y, WHITE, DrawTextureParams {
                    dest_size: Some(vec2(bounds.w, bounds.h)), ..Default::default()
                });
            } else {
                text_in(&art.font, name, bounds, 14, TEXT, true);
            }
            text_in(&art.font, name, rect(130.0, 47.0, 40.0, 15.0), 12, TEXT, true);
            text_in(&art.font, &format!("{:+}%", period.lawful_bonus), rect(130.0, 63.0, 40.0, 17.0), 14, TEXT, true);
        }
        art.frame.status.draw(rect(0.0, 90.0, 78.0, 78.0), s);
        art.frame.unit_report.draw_headings(frame, &art.dialogue_font);
        if let Some(view) = observation
            && let Some(unit) = unit_report::displayed_unit(view, hovered, movement.selected)
        {
            unit_report::draw(frame, art, unit, hovered.unwrap_or(unit.position), movement);
        }
    }
    let hint = Rect::new(frame.sidebar.x + 8.0 * s, frame.end_turn.y - 22.0 * s, frame.sidebar.w - 16.0 * s, 19.0 * s);
    if let Some(message) = movement.message {
        text_in(&art.font, message, hint, 14, ERROR, false);
    } else if movement.menu.is_none() && movement.actions.iter().any(|a| matches!(a, Action::Disband { .. })) {
        text_in(&art.font, "Dismiss veteran [D]", hint, 14, TEXT, false);
    }
}

fn draw_reports(frame: &BattlefieldFrame, state: MatchState, view: Option<&Observation>, economy: Option<engine_lite::income::EconomyReport>, art: &mut BoardArt) {
    let s = frame.scale;
    let rect = |x, w| Rect::new(x * s, 3.0 * s, w * s, 20.0 * s);
    let narrow = frame.compact;
    let x = if narrow { 113.0 } else { 205.0 };
    let mut text_x = x;
    let status = match state {
        MatchState::Playing(turn) => {
            let flag = art.frame.side_flags.entry(turn.side).or_insert_with(|| {
                let palette = art.flag_colors.as_ref().map(|colors| (colors, turn.side));
                load_texture(&art.flag_dir.join("flag-icon.png"), palette)
            });
            paint(flag, Rect::new((x + 6.0) * s, 5.0 * s, 25.0 * s, 17.0 * s));
            text_x += 35.0;
            economy.and_then(|report| report.last_round).map_or_else(||turn.round.to_string(),
                |limit| format!("{}/{limit}", turn.round))
        }
        MatchState::Won(side) => format!("Side {side} won"),
        MatchState::AlliedVictory(sides) => format!("Sides {} won", sides.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")),
        MatchState::CampaignVictory => "Campaign victory".into(),
        MatchState::CampaignDefeat => "Campaign defeat".into(),
        MatchState::Draw => "Draw".into(),
    };
    let icons = if narrow { [215.0, 282.0, 344.0, 400.0, 475.0] } else { [319.0, 407.0, 490.0, 568.0, 666.0] };
    text_in(&art.font, &status, rect(text_x, icons[0] - text_x - 6.0), 15, TEXT, false);
    if let (Some(view), Some(economy)) = (view, economy) {
        let units = view.units.iter().filter(|unit| unit.side == view.side).count();
        let values = [view.gold.map(|v| v.to_string()), Some(format!("{}/{}",economy.villages,economy.known_villages)),
            Some(units.to_string()), Some(format!("{} ({})",economy.expenses,economy.upkeep)), Some(economy.income.to_string())];
        for (index, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                let left = icons[index];
                let icon_width = if index < 3 {17.0} else {25.0};
                paint(&art.frame.reports[index], Rect::new(left * s, 5.0 * s, icon_width * s, 17.0 * s));
                let text_width = icons.get(index+1).copied().unwrap_or(left+83.0)-left-icon_width-5.0;
                let color = if matches!(index,0|4) && value.starts_with('-') { Color::from_rgba(255,96,96,255) } else {TEXT};
                text_in(&art.font, &value, rect(left + icon_width + 4.0, text_width), 15, color, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_theme_keeps_the_original_fixed_panels_at_each_resolution() {
        for (width, height, map_width, mini, end) in [
            (1024.0, 768.0, 842.0, (855.0, 11.0, 160.0, 212.0), (851.0, 718.0, 168.0, 44.0)),
            (1280.0, 900.0, 1098.0, (1111.0, 11.0, 160.0, 212.0), (1107.0, 850.0, 168.0, 44.0)),
            (1024.0, 600.0, 842.0, (885.0, 10.0, 130.0, 158.0), (884.0, 576.0, 80.0, 22.0)),
            (800.0, 600.0, 618.0, (661.0, 10.0, 130.0, 158.0), (660.0, 576.0, 80.0, 22.0)),
        ] {
            let frame = BattlefieldFrame::new(width, height);
            assert_eq!(frame.map, Viewport { x: 0.0, y: 27.0, width: map_width, height: height - 27.0 });
            assert_eq!(frame.sidebar, Rect::new(map_width, 0.0, 182.0, height));
            assert_eq!((frame.minimap.x, frame.minimap.y, frame.minimap.width, frame.minimap.height), mini);
            assert_eq!(frame.end_turn, Rect::new(end.0, end.1, end.2, end.3));
            assert!(frame.menu().bottom() < frame.end_turn.y);
            assert!(frame.map.contains([map_width - 1.0, 27.0]));
            assert!(!frame.map.contains([map_width, 27.0]));
            assert!(!frame.map.contains([0.0, 26.0]));
        }
    }
}

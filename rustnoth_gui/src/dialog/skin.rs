use super::*;
use crate::dialog;

pub(crate) struct Skin {
    pub(crate) font: Option<Font>,
    panel: Option<Texture2D>,
    pub(crate) selected: Option<Texture2D>,
    buttons: [Option<Texture2D>; 2],
}

impl Skin {
    pub(crate) fn load(root: &Path) -> Self {
        let image = |name: &str| crate::render::load_texture(&root.join("images").join(name), None);
        Self {
            font: std::fs::read(root.join("fonts/Lato-Regular.ttf"))
                .ok()
                .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok()),
            panel: image("dialogs/opaque-background.png"),
            selected: image("dialogs/selection-background-fancy.png"),
            buttons: [
                image("buttons/large-button.png"),
                image("buttons/large-button-active.png"),
            ],
        }
    }
    pub(crate) fn paint(tex: &Option<Texture2D>, r: Rect) {
        if let Some(tex) = tex {
            draw_texture_ex(
                tex,
                r.x,
                r.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(r.size()),
                    ..Default::default()
                },
            );
        }
    }
    pub(crate) fn frame(&self, title: &str, w: f32, h: f32, backdrop: &Texture2D) -> (Rect, Vec2) {
        set_default_camera();
        draw_texture_ex(
            backdrop,
            0.0,
            0.0,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(screen_width(), screen_height())),
                flip_y: true,
                ..Default::default()
            },
        );
        draw_rectangle(
            0.0,
            0.0,
            screen_width(),
            screen_height(),
            Color::from_rgba(0, 0, 0, 125),
        );
        let scale = (screen_width() / (w + 16.0))
            .min(screen_height() / (h + 16.0))
            .clamp(0.01, 1.0);
        let size = vec2(screen_width(), screen_height()) / scale;
        set_camera(&dialog::setup_camera(size.x, size.y));
        let r = Rect::new((size.x - w) * 0.5, (size.y - h) * 0.5, w, h);
        draw_rectangle(r.x, r.y, w, h, Color::from_rgba(31, 34, 40, 255));
        if let Some(tex) = &self.panel {
            let tile = tex.size();
            let mut y = 0.0;
            while y < h {
                let mut x = 0.0;
                while x < w {
                    let cut = Rect::new(0.0, 0.0, tile.x.min(w - x), tile.y.min(h - y));
                    draw_texture_ex(
                        tex,
                        r.x + x,
                        r.y + y,
                        WHITE,
                        DrawTextureParams {
                            source: Some(cut),
                            ..Default::default()
                        },
                    );
                    x += tile.x;
                }
                y += tile.y;
            }
        }
        draw_rectangle_lines(r.x, r.y, w, h, 2.0, dialog::GOLD);
        dialog::text(
            self.font.as_ref(),
            title,
            r.x + 18.0,
            r.y + 38.0,
            28,
            dialog::GOLD,
        );
        (r, Vec2::from(mouse_position()) / scale)
    }
    pub(crate) fn button(&self, r: Rect, p: Vec2, label: &str, enabled: bool) -> bool {
        let hover = enabled && r.contains(p);
        Self::paint(&self.buttons[usize::from(hover)], r);
        let m = measure_text(label, self.font.as_ref(), 17, 1.0);
        dialog::text(
            self.font.as_ref(),
            label,
            r.center().x - m.width / 2.0,
            r.center().y + 6.0,
            17,
            if enabled { dialog::GOLD } else { GRAY },
        );
        hover && is_mouse_button_pressed(MouseButton::Left)
    }
}

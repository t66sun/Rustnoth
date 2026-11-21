use super::{Art, LINKS};
use crate::dialog;
use macroquad::prelude::*;

pub(super) const GOLD: [u8; 3] = [186, 172, 125];
pub(super) const WHITE_TEXT: [u8; 3] = [215; 3];
const DARK_BORDER: Color = Color::from_rgba(114, 79, 46, 255);
const EDGE: Color = Color::from_rgba(16, 22, 35, 255);
pub(super) const SHADE: Color = Color::from_rgba(0, 0, 0, 89);

pub(super) fn inset(r: Rect, n: f32) -> Rect {
    Rect::new(r.x + n, r.y + n, r.w - 2. * n, r.h - 2. * n)
}

pub(super) fn fill(r: Rect, color: Color) {
    draw_rectangle(r.x, r.y, r.w, r.h, color);
}

fn outline(r: Rect, color: Color) {
    draw_rectangle(r.x, r.y, r.w, 1., color);
    draw_rectangle(r.x, r.bottom() - 1., r.w, 1., color);
    draw_rectangle(r.x, r.y + 1., 1., r.h - 2., color);
    draw_rectangle(r.right() - 1., r.y + 1., 1., r.h - 2., color);
}

pub(super) fn window(r: Rect, tile: &Texture2D) {
    outline(r, EDGE);
    outline(inset(r, 1.), DARK_BORDER);
    super::super::preferences::tiled(inset(r, 2.), tile);
}

pub(super) fn clip(r: Option<Rect>, scale: f32) {
    let ratio = scale * screen_dpi_scale();
    let bounds = r.map(|r| ((r.x * ratio).round() as i32, (r.y * ratio).round() as i32,
        (r.w * ratio).round() as i32, (r.h * ratio).round() as i32));
    unsafe { get_internal_gl().quad_gl.scissor(bounds); }
}

pub(super) fn highlight(r: Rect, selected: bool, image: &Texture2D) {
    let inside = if selected {
        outline(r, Color::from_rgba(185, 158, 75, 255));
        inset(r, 1.)
    } else {
        r
    };
    super::super::image(inside, image);
}

// GUI2 positions text by its Pango layout, including its ascent and descent.
pub(super) struct Label {
    texture: Option<Texture2D>,
    lines: Vec<String>,
    font: Option<Font>,
    point: u16,
    color: Color,
    pub size: Vec2,
}

impl Label {
    #[cfg(windows)]
    fn pixels(pixels: Image) -> Self {
        let texture = Texture2D::from_image(&pixels);
        texture.set_filter(FilterMode::Nearest);
        Self { size: texture.size(), texture: Some(texture), lines: Vec::new(),
            font: None, point: 0, color: WHITE }
    }

    pub fn new(art: &Art, font: Option<&Font>, text: &str, point: u16, rgb: [u8; 3]) -> Self {
        #[cfg(windows)]
        if let Some(renderer) = &art.text_renderer {
            return Self::pixels(renderer.button(text, point, rgb));
        }
        let _ = art;
        Self::lines(font, vec![text.into()], point, rgb)
    }

    fn paragraph(art: &Art, msg: &str, width: f32) -> Self {
        #[cfg(windows)]
        if let Some(renderer) = &art.text_renderer {
            return Self::pixels(renderer.render(msg, width as u16, false, true));
        }
        let font = art.font.as_ref();
        let msg = msg.replace("<b>", "").replace("</b>", "")
            .replace("<i>", "").replace("</i>", "");
        let lines = msg.split('\n').flat_map(|line| {
            if line.is_empty() { vec![String::new()] } else {
                super::super::wrap_at(line, font, width, 17)
            }
        }).collect();
        Self::lines(font, lines, 17, WHITE_TEXT)
    }

    fn lines(font: Option<&Font>, lines: Vec<String>, point: u16, rgb: [u8; 3]) -> Self {
        let width = lines.iter().map(|s| measure_text(s, font, point, 1.).width.ceil())
            .fold(0., f32::max);
        let height = f32::from(point) + 4. + lines.len().saturating_sub(1) as f32 * 27.;
        Self { texture: None, lines, font: font.cloned(), point, size: vec2(width, height),
            color: Color::from_rgba(rgb[0], rgb[1], rgb[2], 255) }
    }

    pub fn draw(&self, at: Vec2) {
        if let Some(texture) = &self.texture {
            draw_texture(texture, at.x, at.y, WHITE);
        } else {
            for (i, line) in self.lines.iter().enumerate() {
                dialog::text(self.font.as_ref(), line, at.x, at.y + f32::from(self.point) + i as f32 * 27.,
                    self.point, self.color);
            }
        }
    }

    pub fn draw_cell(&self, at: Vec2, width: f32) {
        if self.size.x <= width {
            self.draw(at);
        } else if let Some(texture) = &self.texture {
            draw_texture_ex(texture, at.x, at.y, WHITE, DrawTextureParams {
                source: Some(Rect::new(0., 0., width, self.size.y)), ..Default::default()
            });
        } else if let Some(line) = self.lines.first() {
            let fitted = dialog::fit(self.font.as_ref(), line.clone(), width, self.point);
            dialog::text(self.font.as_ref(), &fitted, at.x, at.y + f32::from(self.point), self.point, self.color);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub bounds: Rect,
    pub list: Rect,
    pub side: Rect,
    pub choose: Rect,
    pub cancel: Rect,
    pub toggle: Rect,
}

impl Layout {
    pub fn new(view: Vec2, columns: Vec2, rows_height: f32, title_height: f32, toggle_width: f32) -> Self {
        let size = view.min(vec2(900., 600.));
        let pos = ((view - size) * 0.5).floor();
        let bounds = Rect::new(pos.x, pos.y, size.x, size.y);
        let big = view.x >= 1200. && view.y >= 900.;
        let button = if big { vec2(128., 30.) } else { vec2(112., 26.) };
        let cancel = Rect::new(bounds.right() - 18. - button.x,
            bounds.bottom() - 18. - button.y, button.x, button.y);
        let choose = Rect::new(cancel.x - 10. - button.x, cancel.y, button.x, button.y);
        let list_y = bounds.y + 25. + title_height;
        let list_height = choose.y - 10. - list_y;
        let scrollbar = if rows_height > list_height { 15. } else { 0. };
        let list_width = columns.x + columns.y + 40. + scrollbar;
        let list = Rect::new(bounds.x + 15., list_y, list_width.min((bounds.w - 53.) / 2.), list_height);
        let side = Rect::new(list.right() + 10., list.y, bounds.right() - 18. - list.right() - 10., list.h);
        let toggle_height = if big { 20. } else { 18. };
        let toggle = Rect::new(list.x, (choose.center().y - toggle_height/2.).floor() - 1.,
            toggle_width.min(choose.x - 10. - list.x), toggle_height);
        Self { bounds, list, side, choose, cancel, toggle }
    }
}

pub(super) struct Information {
    paragraphs: [Label; 2],
    urls: [Label; 2],
    pub width: f32,
}

impl Information {
    pub fn new(art: &Art, width: f32) -> Self {
        let msg = super::INFORMATION.map(|s| art.locale.ui(s));
        let intro = format!("{}\n\n{}", msg[0], msg[1]);
        Self {
            paragraphs: [Label::paragraph(art, &intro, width), Label::paragraph(art, msg[2], width)],
            urls: LINKS.map(|url| Label::new(art, art.font.as_ref(), url, 17, [255, 225, 0])),
            width,
        }
    }

    pub fn height(&self) -> f32 {
        (0..2).map(|i| self.paragraphs[i].size.y + 10. + self.urls[i].size.y + 26.).sum::<f32>() + 20.
    }

    pub fn draw(&self, side: Rect, offset: f32, pointer: Vec2, click: bool) -> std::io::Result<()> {
        let mut y = side.y - offset;
        for (i, label) in self.paragraphs.iter().enumerate() {
            label.draw(vec2(side.x, y));
            y += label.size.y + 10.;
            let url = &self.urls[i];
            let box_rect = Rect::new(side.x, y, self.width, url.size.y + 26.);
            fill(inset(box_rect, 3.), Color::from_rgba(0, 0, 0, 137));
            outline(box_rect, EDGE);
            outline(inset(box_rect, 1.), DARK_BORDER);
            outline(inset(box_rect, 2.), EDGE);
            let x = (box_rect.x + (box_rect.w - url.size.x) * 0.5).floor().max(side.x);
            let pos = vec2(x, box_rect.y + 13.);
            url.draw(pos);
            draw_rectangle(pos.x, pos.y + url.size.y - 2., url.size.x, 1., Color::from_rgba(255, 225, 0, 255));
            if click && side.contains(pointer) && Rect::new(pos.x, pos.y, url.size.x, url.size.y).contains(pointer) {
                super::open_link(LINKS[i])?;
            }
            y = box_rect.bottom() + 20.;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_window_places_the_list_and_confirmation_buttons_in_the_gui2_grid() {
        let layout = Layout::new(vec2(1200., 820.), vec2(202., 44.), 1070., 27., 309.);
        assert_eq!(layout.bounds, Rect::new(150., 110., 900., 600.));
        assert_eq!(layout.list, Rect::new(165., 162., 301., 494.));
        assert_eq!(layout.side.x, 476.);
        assert_eq!(layout.choose, Rect::new(798., 666., 112., 26.));
        assert_eq!(layout.cancel, Rect::new(920., 666., 112., 26.));
        let short = Layout::new(vec2(1200., 820.), vec2(202., 44.), 123., 27., 309.);
        assert_eq!(short.list.w, 286.);
    }
}

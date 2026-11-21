use super::{load_texture, view};
use crate::list_scroll::Scroll;
use macroquad::prelude::*;
use std::{error::Error, path::Path};

pub(super) struct BarArt([[Texture2D; 3]; 3]);

impl BarArt {
    pub fn load(root: &Path) -> Result<Self, Box<dyn Error>> {
        let state = |suffix: &str| -> Result<_, Box<dyn Error>> {
            let part = |name| load_texture(root, &format!("images/buttons/scrollbars/{name}{suffix}.png"));
            Ok([part("scrolltop")?, part("scrollmid")?, part("scrollbottom")?])
        };
        Ok(Self([state("")?, state("-active")?, state("-pressed")?]))
    }

    pub fn update(&self, scroll: &mut Scroll, viewport: Rect, total: f32,
        pointer: Vec2, wheel: f32, scale: f32) {
        let bar = Rect::new(viewport.right() - 15., viewport.y, 15., viewport.h);
        scroll.update(bar, total, pointer, is_mouse_button_pressed(MouseButton::Left),
            is_mouse_button_down(MouseButton::Left), if viewport.contains(pointer) { wheel } else { 0. });
        if total <= viewport.h { return; }
        let thumb = scroll.thumb(bar, total);
        let state = if scroll.grab.is_some() { 2 } else { usize::from(thumb.contains(pointer)) };
        view::clip(Some(bar), scale);
        view::fill(bar, view::SHADE);
        let [top, mid, end] = &self.0[state];
        draw_texture(top, bar.x, thumb.y, WHITE);
        super::super::image(Rect::new(bar.x, thumb.y + 2., bar.w, thumb.h - 2.), mid);
        draw_texture(end, bar.x, thumb.bottom(), WHITE);
        view::clip(None, scale);
    }
}

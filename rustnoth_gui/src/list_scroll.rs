use macroquad::prelude::*;

/// GUI2 lists scroll in pixels independently of the currently selected row.
#[derive(Default)]
pub(super) struct Scroll {
    pub offset: f32,
    pub grab: Option<(f32, f32)>,
}

impl Scroll {
    pub fn thumb(&self, bar: Rect, content: f32) -> Rect {
        let length = (bar.h * bar.h / content.max(bar.h)).floor().max(11.).min(bar.h);
        let range = (content - bar.h).max(0.);
        let pos = if range > 0. { (self.offset * (bar.h - length) / range).floor() } else { 0. };
        Rect::new(bar.x, bar.y + pos, bar.w, length)
    }

    pub fn reveal(&mut self, top: f32, row_height: f32, view: f32) {
        self.offset = self.offset.clamp((top + row_height - view).max(0.).min(top), top);
    }

    pub fn update(&mut self, bar: Rect, content: f32, point: Vec2, click: bool, down: bool, wheel: f32) {
        let range = (content - bar.h).max(0.);
        self.offset = self.offset.min(range);
        if !down || range == 0. { self.grab = None; }
        let thumb = self.thumb(bar, content);
        let half_page = (bar.h / 2.).floor();
        if click && bar.contains(point) {
            if thumb.contains(point) {
                self.grab = Some((point.y, self.offset));
            } else {
                self.offset += if point.y < thumb.y { -half_page } else { half_page };
            }
        }
        if let Some((start_y, start_offset)) = self.grab && bar.h > thumb.h {
            self.offset = (start_offset + (point.y - start_y) * range / (bar.h - thumb.h)).round();
        } else if wheel != 0. {
            self.offset -= wheel.signum() * half_page;
        }
        self.offset = self.offset.clamp(0., range);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_click_pages_and_drag_keeps_the_grab_offset_outside_the_track() {
        let bar = Rect::new(387., 128., 15., 541.);
        let height = 18. * 82.;
        let mut scroll = Scroll::default();
        assert_eq!(scroll.thumb(bar, height).h, 198.);
        scroll.update(bar, height, vec2(395., 400.), true, true, 0.);
        assert_eq!(scroll.offset, 270.);
        let thumb = scroll.thumb(bar, height);
        scroll.update(bar, height, vec2(395., thumb.y + 60.), true, true, 0.);
        scroll.update(bar, height, vec2(250., bar.bottom() - thumb.h + 60.), false, true, 0.);
        assert_eq!(scroll.offset, 935.);
        scroll.update(bar, height, vec2(250., 200.), false, false, 0.);
        assert!(scroll.grab.is_none());
        scroll.reveal(0., 82., bar.h);
        assert_eq!(scroll.offset, 0.);
    }
}

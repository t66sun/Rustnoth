use super::{Text, ptr};
use macroquad::texture::Image;

impl Text {
    fn message_markup(&self, text: &str) -> bool {
        // Bare '&' and '<' are also legal dialogue text. Avoid reusing a previous
        // Pango layout when a WML author supplied plain text rather than markup.
        (text.contains('<') || text.contains('&')) && unsafe {
            (self.api.pango_parse_markup)(text.as_ptr().cast(), text.len() as i32, 0,
                ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) != 0
        }
    }

    pub(crate) fn dialogue_caption(&self, text: &str, width: u16) -> Image {
        unsafe {
            (self.api.pango_layout_set_height)(self.layout, -1);
            (self.api.pango_layout_set_ellipsize)(self.layout, 3); // PANGO_ELLIPSIZE_END
        }
        let (w, h) = self.prepare(text, 22, Some(width), 0, self.message_markup(text));
        let image = self.rasterize(w, h, [186, 172, 125]);
        unsafe { (self.api.pango_layout_set_ellipsize)(self.layout, 0); }
        image
    }

    /// Rasterize a whole label, or just one screenful between shaped lines.
    pub(crate) fn dialogue_body(&self, text: &str, width: u16, available: u16, page: usize)
        -> (Image, bool, bool)
    {
        let (w, h) = self.prepare(text, 17, Some(width), 0, self.message_markup(text));
        if h <= available {
            return (self.rasterize(w, h, [215; 3]), false, true);
        }
        // Leave the existing confirmation button below a paginated message.
        let limit = i32::from(available.saturating_sub(31).max(1));
        let mut breaks = vec![0];
        unsafe {
            let iter = (self.api.pango_layout_get_iter)(self.layout);
            loop {
                let (mut top, mut end) = (0, 0);
                (self.api.pango_layout_iter_get_line_yrange)(iter, &mut top, &mut end);
                let top = (top + 512) / 1024;
                let end = (end + 512) / 1024;
                let start = *breaks.last().unwrap();
                if end - start > limit && top > start { breaks.push(top); }
                if (self.api.pango_layout_iter_next_line)(iter) == 0 {
                    breaks.push(end.max(top + 1));
                    break;
                }
            }
            (self.api.pango_layout_iter_free)(iter);
        }
        let page = page.min(breaks.len() - 2);
        let start = breaks[page];
        let height = (breaks[page + 1] - start).clamp(1, limit) as u16;
        (self.rasterize_label(w, height, [215; 3], start, false), true, page + 2 == breaks.len())
    }
}

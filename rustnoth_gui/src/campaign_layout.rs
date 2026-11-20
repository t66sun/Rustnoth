use macroquad::prelude::*;

pub(super) struct Layout {
    pub window: Rect,
    pub panel: Rect,
    pub list: Rect,
    pub details: Rect,
    pub search: Rect,
    pub play: Rect,
    pub cancel: Rect,
}

impl Layout {
    pub fn new(w: f32, h: f32, name_width: f32) -> Self {
        // GUI2's default window has 10/13 px borders. The three option
        // columns share the remaining width using integer pixels.
        let width = w.min(1046.).floor();
        let height = h.min(750.).floor();
        let window = Rect::new(((w-width)/2.).floor(), ((h-height)/2.).floor(), width, height);
        let panel = Rect::new(window.x+15., window.y+55., width-33., height-163.);
        let list_width = (name_width+108.).clamp(200., panel.w*0.48).floor();
        let list = Rect::new(panel.x+8., panel.y+8., list_width, panel.h-16.);
        let details = Rect::new(list.right()+10., list.y, panel.right()-list.right()-18., list.h);
        let search = Rect::new(panel.right()-250., window.y+15., 250., 30.);
        let play = Rect::new(panel.right()-294., window.bottom()-58., 172., 40.);
        let cancel = Rect::new(panel.right()-112., play.y+7., 112., 26.);
        Self { window, panel, list, details, search, play, cancel }
    }
}

pub(super) fn portrait(pane: Rect, text_h: f32, size: Vec2, scroll: f32) -> Rect {
    let top = pane.y + 15. + text_h;
    let spare = (pane.bottom() - 5. - top - size.y).max(0.);
    Rect::new((pane.center().x - size.x/2.).floor(), (top + spare/2.).floor() - scroll, size.x, size.y)
}

// The WFL dimensions in panel_campaign_menu use integer division.
pub(super) fn backdrop(pane: Rect, size: Vec2) -> Rect {
    let ratio = (size.x / size.y).floor();
    if ratio < 1. {
        return Rect::new(pane.x+(pane.w/2.).floor(), pane.y, size.x, size.y);
    }
    let width = (pane.w / ratio).floor();
    Rect::new(pane.x+((pane.w-width)/2.).floor(), pane.y, width, pane.h*ratio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_selection_places_the_tutorial_portrait_below_its_description() {
        let layout = Layout::new(1200., 820., 194.);
        assert_eq!(layout.panel, Rect::new(92.,90.,1013.,587.));
        assert_eq!(layout.list, Rect::new(100.,98.,302.,571.));
        assert_eq!(layout.details, Rect::new(412.,98.,685.,571.));
        assert_eq!(layout.search, Rect::new(855.,50.,250.,30.));
        assert_eq!(layout.play, Rect::new(811.,727.,172.,40.));
        assert_eq!(portrait(layout.details,21.,vec2(350.,350.),0.), Rect::new(579.,224.,350.,350.));
        assert_eq!(backdrop(layout.panel,vec2(1300.,970.)), layout.panel);
    }
}

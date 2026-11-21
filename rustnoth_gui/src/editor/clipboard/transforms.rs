//! Clipboard transforms from the classic editor tool panel and editor hotkeys.
use super::{Clipboard, Hex};
use crate::editor::{Art, AssetCatalog};
use macroquad::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transform { Horizontal, Vertical, Clockwise, CounterClockwise }

use Transform::*;

const BUTTONS: [(Transform, &str, &str); 4] = [
    (Horizontal, "flip-horizontal", "Flip Clipboard Horizontally"),
    (Vertical, "flip-vertical", "Flip Clipboard Vertically"),
    (Clockwise, "rotate-cw", "Rotate Clipboard Clockwise"),
    (CounterClockwise, "rotate-ccw", "Rotate Clipboard Counter-Clockwise"),
];

impl Clipboard {
    fn transform(&mut self, action: Transform) {
        for (offset, _) in &mut self.0 {
            match action {
                Horizontal => offset.x = -offset.x,
                Vertical => offset.y = -offset.y - (offset.x & 1),
                Clockwise | CounterClockwise => {
                    // Convert staggered columns to axial coordinates around the paste origin.
                    let q = offset.x;
                    let r = offset.y - q.div_euclid(2);
                    let (x, row) = if action == Clockwise { (-r, q + r) } else { (q + r, -q) };
                    *offset = Hex { x, y: row + x.div_euclid(2) };
                }
            }
        }
        if matches!(action, Horizontal | Vertical) {
            self.center_by_mass();
        }
    }
}

fn slot(panel: Rect, compact: bool, index: usize) -> Option<Rect> {
    let i = index as f32;
    if compact {
        // 1024x600 / 800x600 have a vertical stack beside the first five tools.
        (index < 3).then(|| Rect::new(panel.x + 146., panel.y + 6. + i * 26., 25., 25.))
    } else {
        (index < 5).then(|| Rect::new(panel.x + 14. + i * 32., panel.y + 85., 30., 30.))
    }
}

#[derive(Default)]
pub(in crate::editor) struct TransformButtons {
    pressed: Option<Transform>,
}

impl TransformButtons {
    pub(in crate::editor) fn draw(&mut self, panel: Rect, compact: bool, pointer: Vec2, pasting: bool,
            clipboard: &mut Clipboard, art: &mut Art, assets: &AssetCatalog, tip: &mut Option<&'static str>) {
        let enabled = pasting && !clipboard.empty();
        let modifier = if cfg!(target_os = "macos") {
            is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper)
        } else {
            is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
        };
        if enabled && modifier && !is_key_down(KeyCode::LeftAlt) && !is_key_down(KeyCode::RightAlt) && is_key_pressed(KeyCode::R) {
            let backwards = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            clipboard.transform(if backwards { CounterClockwise } else { Clockwise });
        }

        let mut hovered = None;
        for (i, definition) in BUTTONS.iter().copied().map(Some).chain(std::iter::once(None)).enumerate() {
            let Some(rect) = slot(panel, compact, i) else { continue };
            let size = rect.w as u8;
            let hot = enabled && definition.is_some() && rect.contains(pointer);
            let mut state = "";
            if hot && let Some((action, _, label)) = definition {
                hovered = Some(action);
                *tip = Some(label);
                if is_mouse_button_pressed(MouseButton::Left) { self.pressed = Some(action); }
                state = if self.pressed == Some(action) { "-pressed" } else { "-active" };
            }
            let gray = if enabled && definition.is_some() { "" } else { "~GS()" };
            art.image(assets, &format!("buttons/button_square/button_square_{size}{state}.png{gray}"), rect);
            if let Some((_, icon, _)) = definition {
                art.image(assets, &format!("icons/action/editor-clipboard-{icon}_{size}{state}.png{gray}"), rect);
            }
        }
        if is_mouse_button_released(MouseButton::Left) {
            if let Some(action) = self.pressed.take().filter(|a| Some(*a) == hovered) {
                clipboard.transform(action);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coords(c: &Clipboard) -> Vec<(i32, i32, &str)> {
        c.translated(Hex { x: 0, y: 0 }).map(|(at, code)| (at.x, at.y, code)).collect()
    }

    #[test]
    fn clipboard_rotation_keeps_the_paste_origin_and_reflections_recenter_by_mass() {
        let shape = || Clipboard(vec![
            (Hex { x: 0, y: 0 }, "Gg".into()), (Hex { x: 1, y: 1 }, "Ww".into()),
            (Hex { x: 2, y: -1 }, "Mm".into()),
        ]);
        let mut c = shape();
        c.transform(Clockwise);
        assert_eq!(coords(&c), [(0,0,"Gg"),(-1,1,"Ww"),(2,1,"Mm")]);
        c.transform(CounterClockwise);
        assert_eq!(coords(&c), coords(&shape()));
        c.transform(Horizontal);
        assert_eq!(coords(&c), [(1,-1,"Gg"),(0,1,"Ww"),(-1,-2,"Mm")]);
        let mut c = shape();
        c.transform(Vertical);
        assert_eq!(coords(&c), [(-1,-1,"Gg"),(0,-2,"Ww"),(1,0,"Mm")]);
    }

    #[test]
    fn large_and_compact_editor_themes_use_their_own_clipboard_slots() {
        let wide = Rect::new(1019., 304., 181., 155.);
        assert_eq!(slot(wide, false, 0), Some(Rect::new(1033.,389.,30.,30.)));
        assert_eq!(slot(wide, false, 4), Some(Rect::new(1161.,389.,30.,30.)));
        let small = Rect::new(619.,219.,181.,92.);
        assert_eq!(slot(small, true, 2), Some(Rect::new(765.,277.,25.,25.)));
        assert_eq!(slot(small, true, 3), None);
    }
}

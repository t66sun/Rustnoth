//! The six controls in Wesnoth's wide and compact minimap themes.
use crate::{camera::BoardCamera, display_preferences::{MiniLayer, MiniLayers}, render::{self, EditorMinimap}};
use macroquad::prelude::*;
use std::{io, path::Path};

const CONTROLS: [(Option<MiniLayer>, &str, &str); 6] = [
    (None, "zoomdefault", "Default Zoom"),
    (Some(MiniLayer::Terrain), "minimap-draw-terrain", "Toggle Minimap Terrain Drawing"),
    (Some(MiniLayer::Units), "editor-tool-unit", "Toggle Minimap Unit Drawing"),
    (Some(MiniLayer::Villages), "editor-tool-village", "Toggle Minimap Village Drawing"),
    (Some(MiniLayer::TeamColors), "minimap-unit-coding", "Toggle Minimap Unit Coding"),
    (Some(MiniLayer::TerrainImages), "minimap-terrain-coding", "Toggle Minimap Terrain Coding"),
];

pub(crate) struct Buttons {
    previous_zoom: f32,
    backgrounds: [Option<Texture2D>; 4],
    icons: [[Option<Texture2D>; 3]; 6],
}

fn bounds(frame: Rect, index: usize) -> Rect {
    let scale = frame.w / 181.;
    let (x, y) = if frame.h < frame.w {
        (15., 6. + 26. * index as f32)
    } else {
        (14. + 26. * index as f32 + if index == 0 { 0. } else { 2. }, 220.)
    };
    Rect::new(frame.x + x * scale, frame.y + y * scale, 25. * scale, 25. * scale)
}

fn hit(frame: Rect, point: Vec2) -> Option<usize> {
    (0..CONTROLS.len()).find(|&index| bounds(frame, index).contains(point))
}

fn default_zoom(zoom: f32) -> bool { (zoom - 72.).abs() < 0.01 }

impl Buttons {
    pub(crate) fn load(root: &Path) -> Self {
        let images = root.join("images");
        let read = |path: String| render::load_texture(&images.join(path), None);
        Self {
            previous_zoom: 36.,
            backgrounds: ["", "-active", "-pressed", "-active-pressed"].map(|state|
                read(format!("buttons/button_square/button_square_25{state}.png"))),
            icons: CONTROLS.map(|(_, id, _)| ["", "-active", "-pressed"].map(|state|
                read(format!("icons/action/{id}_25{state}.png")))),
        }
    }

    pub(crate) fn draw(&self, frame: Rect, point: Option<Vec2>, zoom: f32, layers: MiniLayers) -> Option<&'static str> {
        let hovered = point.and_then(|point| hit(frame, point));
        for (index, (layer, _, _)) in CONTROLS.iter().enumerate() {
            let pressed = layer.map_or(default_zoom(zoom), |layer| layers.enabled(layer));
            let active = hovered == Some(index);
            let rect = bounds(frame, index);
            let background = 2 * usize::from(pressed) + usize::from(active);
            let icon = if pressed { 2 } else { usize::from(active) };
            for texture in [&self.backgrounds[background], &self.icons[index][icon]].into_iter().flatten() {
                draw_texture_ex(texture, rect.x, rect.y, WHITE, DrawTextureParams {
                    dest_size: Some(rect.size()), ..Default::default()
                });
            }
        }
        hovered.map(|index| CONTROLS[index].2)
    }

    pub(crate) fn input(&mut self, frame: Rect, point: Vec2, camera: &mut BoardCamera, mini: &mut EditorMinimap) -> io::Result<bool> {
        let pressed = is_mouse_button_pressed(MouseButton::Left).then_some(point);
        self.input_press(frame, pressed, camera, mini)
    }

    pub(crate) fn input_press(&mut self, frame: Rect, pressed: Option<Vec2>, camera: &mut BoardCamera, mini: &mut EditorMinimap) -> io::Result<bool> {
        let clicked = pressed.and_then(|point| hit(frame, point));
        let zoom_key = is_key_pressed(KeyCode::Key0) && ![
            KeyCode::LeftControl, KeyCode::RightControl, KeyCode::LeftAlt, KeyCode::RightAlt,
            KeyCode::LeftShift, KeyCode::RightShift, KeyCode::LeftSuper, KeyCode::RightSuper,
        ].into_iter().any(is_key_down);
        let Some(index) = clicked.or_else(|| zoom_key.then_some(0)) else { return Ok(false) };
        if let Some(layer) = CONTROLS[index].0 {
            mini.toggle(layer)?;
        } else if let Some([x, y, zoom]) = camera.checkpoint() {
            let target = if default_zoom(zoom) { self.previous_zoom } else {
                self.previous_zoom = zoom;
                72.
            };
            camera.restore([x, y, target]);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimap_buttons_follow_the_wide_row_and_compact_column() {
        let wide = Rect::new(1019.,6.,181.,255.);
        assert_eq!(bounds(wide,0), Rect::new(1033.,226.,25.,25.));
        assert_eq!(bounds(wide,1), Rect::new(1061.,226.,25.,25.));
        assert_eq!(bounds(wide,5), Rect::new(1165.,226.,25.,25.));
        assert_eq!(bounds(Rect::new(619.,6.,181.,170.),5), Rect::new(634.,142.,25.,25.));
    }

    #[test]
    fn battlefield_buttons_accept_the_same_pixels_that_the_sidebar_draws() {
        for frame in [Rect::new(1018.,6.,181.,255.), Rect::new(618.,6.,181.,170.), Rect::new(309.,3.,90.5,85.)] {
            for (index, (layer, _, _)) in CONTROLS.iter().enumerate() {
                let point = bounds(frame, index).center();
                assert_eq!(hit(frame, point), Some(index));
                assert_eq!(*layer, index.checked_sub(1).map(|i| MiniLayer::ALL[i]));
            }
            assert_eq!(hit(frame, frame.point()), None);
        }
    }
}

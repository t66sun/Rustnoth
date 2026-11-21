use super::{Art, AssetCatalog, BoardCamera, EditorMinimap};
use crate::display_preferences::MiniLayer;
use macroquad::prelude::*;

pub(super) struct Buttons { previous_zoom: f32 }

impl Default for Buttons {
    fn default() -> Self { Self { previous_zoom: 36.0 } }
}

fn bounds(frame: Rect, index: usize) -> Rect {
    let (x,y) = if frame.h == 170. {
        (frame.x + 15., frame.y + 6. + 26. * index as f32)
    } else {
        let spacing = if index == 0 { 0. } else { 2. };
        (frame.x + 14. + 26. * index as f32 + spacing, frame.y + 220.)
    };
    Rect::new(x, y, 25., 25.)
}

impl Buttons {
    pub fn draw(&mut self, frame: Rect, point: Vec2, active: bool, camera: &mut BoardCamera,
                mini: &mut EditorMinimap, art: &mut Art, assets: &AssetCatalog,
                tip: &mut Option<&'static str>) -> std::io::Result<()> {
        let controls = [
            (None, "zoomdefault", "Default Zoom"),
            (Some(MiniLayer::Terrain), "minimap-draw-terrain", "Toggle Minimap Terrain Drawing"),
            (Some(MiniLayer::Units), "editor-tool-unit", "Toggle Minimap Unit Drawing"),
            (Some(MiniLayer::Villages), "editor-tool-village", "Toggle Minimap Village Drawing"),
            (Some(MiniLayer::TeamColors), "minimap-unit-coding", "Toggle Minimap Unit Coding"),
            (Some(MiniLayer::TerrainImages), "minimap-terrain-coding", "Toggle Minimap Terrain Coding"),
        ];
        for (index, (layer, icon, text)) in controls.into_iter().enumerate() {
            let rect = bounds(frame, index);
            let hovered = active && rect.contains(point);
            let selected = match layer {
                Some(layer) => mini.layers.enabled(layer),
                None => camera.layout().is_some_and(|layout| (layout.tile_size()-72.).abs() < 0.01),
            };
            let state = match (selected, hovered) {
                (true,true) => "-active-pressed",
                (true,false) => "-pressed",
                (false,true) => "-active",
                (false,false) => "",
            };
            let overlay = if selected { "-pressed" } else if hovered { "-active" } else { "" };
            art.image(assets, &format!("buttons/button_square/button_square_25{state}.png"), rect);
            art.image(assets, &format!("icons/action/{icon}_25{overlay}.png"), rect);
            if hovered {
                *tip = Some(text);
            }
            let zoom_key = layer.is_none() && active && is_key_pressed(KeyCode::Key0)
                && ![KeyCode::LeftControl, KeyCode::RightControl, KeyCode::LeftAlt, KeyCode::RightAlt,
                     KeyCode::LeftShift, KeyCode::RightShift, KeyCode::LeftSuper, KeyCode::RightSuper]
                    .into_iter().any(is_key_down);
            if (hovered && is_mouse_button_pressed(MouseButton::Left)) || zoom_key {
                if let Some(layer) = layer {
                    mini.toggle(layer)?;
                } else if let Some([x,y,zoom]) = camera.checkpoint() {
                    let target = if selected { self.previous_zoom } else {
                        self.previous_zoom = zoom;
                        72.
                    };
                    camera.restore([x,y,target]);
                }
            }
        }
        Ok(())
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
}

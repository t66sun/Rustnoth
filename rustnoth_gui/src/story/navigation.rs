use crate::{artwork, dialog, title_screen::button_canvas::{self, State}};
use macroquad::prelude::*;
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action { Back, Next, Skip }

// The pair of 56px arrow buttons shares a centered row above Skip.
fn buttons(panel: Rect, screen: Vec2, label_width: f32) -> [(Action, Rect); 3] {
    let big = screen.x >= 1200. && screen.y >= 900.;
    let (width, height, padding): (f32, f32, f32) = if big { (128., 30., 16.) } else { (112., 26., 13.) };
    let width = width.max(label_width + padding);
    let skip = Rect::new(panel.right() - 20. - width, panel.bottom() - 20. - height, width, height);
    let left = (skip.center().x - 56.).floor();
    let y = skip.y - 36.;
    [(Action::Back, Rect::new(left, y, 56., 26.)),
     (Action::Next, Rect::new(left + 56., y, 56., 26.)),
     (Action::Skip, skip)]
}

fn clicked(pressed: &mut Option<Action>, hover: Option<Action>, down: bool, up: bool) -> Option<Action> {
    if down { *pressed = hover; }
    if up && pressed.take() == hover { return hover; }
    None
}

fn textures(root: &Path, stem: &str) -> [Option<Texture2D>; 4] {
    std::array::from_fn(|i| {
        let suffix = ["", "-active", "-pressed", ""][i];
        let path = root.join("images").join(format!("{stem}{suffix}.png"));
        let mut image = artwork::image(&fs::read(path).ok()?).ok()?;
        if i == 3 {
            for [r,g,b,_] in image.get_image_data_mut() {
                let gray = ((77*u16::from(*r) + 150*u16::from(*g) + 29*u16::from(*b)) / 256) as u8;
                (*r,*g,*b) = (gray,gray,gray);
            }
        }
        let tex = Texture2D::from_image(&image);
        tex.set_filter(FilterMode::Nearest);
        Some(tex)
    })
}

pub(super) struct Navigation {
    backgrounds: [Option<Texture2D>; 4],
    arrows: [[Option<Texture2D>; 4]; 2],
    label: Option<Texture2D>,
    pressed: Option<Action>,
}

impl Navigation {
    pub fn new(root: &Path, label: Option<Image>) -> Self {
        Self {
            backgrounds: textures(root, "buttons/button_normal/background"),
            arrows: ["left", "right"].map(|direction| {
                textures(root, &format!("icons/arrows/long_arrow_ornate_{direction}"))
            }),
            label: label.as_ref().map(Texture2D::from_image),
            pressed: None,
        }
    }

    pub fn draw(&mut self, panel: Rect, screen: Vec2, pointer: Vec2, can_back: bool,
                font: Option<&Font>, skip: &str) -> Option<Action> {
        let label_width = self.label.as_ref().map_or_else(|| measure_text(skip, font, 16, 1.).width, Texture2D::width);
        let controls = buttons(panel, screen, label_width);
        let hover = controls.iter().find(|(action, bounds)| {
            (*action != Action::Back || can_back) && bounds.contains(pointer)
        }).map(|(action, _)| *action);
        let action = clicked(&mut self.pressed, hover, is_mouse_button_pressed(MouseButton::Left),
            is_mouse_button_released(MouseButton::Left));
        for (control, rect) in controls {
            let (index, state) = if control == Action::Back && !can_back { (3, State::Disabled) }
                else if hover != Some(control) { (0, State::Enabled) }
                else if self.pressed == hover { (2, State::Pressed) }
                else { (1, State::Focused) };
            if let Some(tex) = &self.backgrounds[index] {
                if control == Action::Skip { button_canvas::draw(rect, tex, state); }
                else { button_canvas::icon_button(rect, tex, state); }
            }
            if control == Action::Skip {
                if let Some(tex) = &self.label {
                    let x = rect.x + ((rect.w-tex.width())/2.).floor().max(0.);
                    let y = rect.y + ((rect.h-tex.height()-2.)/2.).floor().max(0.);
                    draw_texture(tex, x, y, WHITE);
                } else {
                    let text = measure_text(skip, font, 16, 1.);
                    dialog::text(font, skip, rect.center().x - text.width/2.,
                        rect.center().y + text.height/2. - 1., 16, Color::from_rgba(186,172,125,255));
                }
            } else if let Some(tex) = &self.arrows[usize::from(control == Action::Next)][index] {
                let offset = ((rect.size() - tex.size()) / 2.).floor().max(Vec2::ZERO);
                draw_texture(tex, rect.x + offset.x, rect.y + offset.y, WHITE);
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn story_navigation_uses_the_stock_two_row_grid_and_same_button_release() {
        let screen = vec2(1200.,820.);
        let panel = Rect::new(0.,615.,1200.,205.);
        assert_eq!(buttons(panel, screen, 26.), [
            (Action::Back, Rect::new(1068.,738.,56.,26.)),
            (Action::Next, Rect::new(1124.,738.,56.,26.)),
            (Action::Skip, Rect::new(1068.,774.,112.,26.))]);
        let mut held = None;
        assert_eq!(clicked(&mut held, Some(Action::Next), true, false), None);
        assert_eq!(clicked(&mut held, Some(Action::Skip), false, true), None);
        clicked(&mut held, Some(Action::Skip), true, false);
        assert_eq!(clicked(&mut held, Some(Action::Skip), false, true), Some(Action::Skip));
    }
}

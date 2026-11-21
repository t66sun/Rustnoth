//! Wesnoth's Preferences window, with the General and Display pages.
use super::{Art, dialog, load_texture, version};
use crate::display_preferences::{self, speed::{FACTORS, Speed}};
use macroquad::prelude::*;
use std::{error::Error, io, path::Path};

const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
const LABEL: Color = Color::from_rgba(215, 215, 215, 255);
const BORDER: Color = Color::from_rgba(114, 79, 46, 255);
const TABS: [&str; 6] = ["General", "Hotkeys", "Display", "Sound", "Multiplayer", "Advanced"];

pub(super) fn tiled(rect: Rect, texture: &Texture2D) {
    let mut y = rect.y;
    while y < rect.bottom() {
        let mut x = rect.x;
        while x < rect.right() {
            let w = texture.width().min(rect.right() - x);
            let h = texture.height().min(rect.bottom() - y);
            draw_texture_ex(texture, x, y, WHITE, DrawTextureParams {
                source: Some(Rect::new(0.0, 0.0, w, h)), ..Default::default()
            });
            x += w;
        }
        y += texture.height();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page { General, Display }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control { GeneralTab, DisplayTab, Scroll, Accelerated, Factor, Fullscreen, Grid, About, Close }

impl Page {
    fn controls(self) -> &'static [Control] {
        use Control::*;
        match self {
            Self::General => &[GeneralTab, DisplayTab, Scroll, Accelerated, Factor, About, Close],
            Self::Display => &[GeneralTab, DisplayTab, Fullscreen, Grid, About, Close],
        }
    }
}

struct Layout { window: Rect, scroll_label_width: f32 }

impl Layout {
    fn new(view: Vec2, scroll_label_width: f32) -> Self {
        // GUI2 centers the window using integer division.
        let size = vec2(793., 610.);
        let pos = ((view - size) * 0.5).floor();
        Self { window: Rect::new(pos.x, pos.y, size.x, size.y), scroll_label_width }
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(self.window.x + x, self.window.y + y, w, h)
    }

    fn tab(&self, index: usize) -> Rect {
        self.rect(15., 52. + 82. * index as f32, 192., 82.)
    }

    fn control(&self, control: Control) -> Rect {
        use Control::*;
        match control {
            GeneralTab => self.tab(0),
            DisplayTab => self.tab(2),
            Scroll => {
                let x = 217. + self.scroll_label_width + 10.;
                self.rect(x, 52., 775. - x, 22.)
            }
            Accelerated => self.rect(217., 122., 557., 19.),
            Factor => self.rect(242., 176., 533., 22.),
            Fullscreen => self.rect(217., 118., 557., 19.),
            Grid => self.rect(217., 303., 557., 19.),
            About => self.rect(15., 566., 25., 25.),
            Close => self.rect(663., 566., 112., 26.),
        }
    }
}

struct WidgetArt {
    backdrop: Texture2D,
    selected: Texture2D,
    icons: Vec<Texture2D>,
    checks: [Texture2D; 4],
    sliders: [Texture2D; 3],
}

impl WidgetArt {
    fn load(root: &Path) -> Result<Self, Box<dyn Error>> {
        let load = |name: &str| load_texture(root, &format!("images/{name}.png"));
        Ok(Self {
            backdrop: load("dialogs/opaque-background")?,
            selected: load("dialogs/selection-background-fancy")?,
            icons: TABS.iter().map(|name| load(&format!("icons/icon-{}", name.to_lowercase())))
                .collect::<Result<_, _>>()?,
            checks: [load("buttons/checkbox")?, load("buttons/checkbox-active")?,
                load("buttons/checkbox-pressed")?, load("buttons/checkbox-active-pressed")?],
            sliders: [load("buttons/sliders/slider")?, load("buttons/sliders/slider-active")?,
                load("buttons/sliders/slider-pressed")?],
        })
    }

    fn frame(&self, layout: &Layout) {
        let r = layout.window;
        draw_rectangle(r.x, r.y, r.w, r.h, Color::from_rgba(16, 22, 35, 255));
        draw_rectangle(r.x + 1., r.y + 1., r.w - 2., r.h - 2., BORDER);
        tiled(Rect::new(r.x + 2., r.y + 2., r.w - 4., r.h - 4.), &self.backdrop);
    }

    fn tabs(&self, art: &Art, layout: &Layout, page: Page, focus: Option<Control>) {
        for (i, (label, icon)) in TABS.iter().zip(&self.icons).enumerate() {
            let (active, target) = match i {
                0 => (page == Page::General, Some(Control::GeneralTab)),
                2 => (page == Page::Display, Some(Control::DisplayTab)),
                _ => (false, None),
            };
            let r = layout.tab(i);
            draw_rectangle(r.x, r.y, r.w, r.h, Color::from_rgba(0, 0, 0, 89));
            if active || (target.is_some() && target == focus) {
                let alpha = if active { 255 } else { 191 };
                draw_texture_ex(&self.selected, r.x, r.y, Color::from_rgba(255, 255, 255, alpha),
                    DrawTextureParams { dest_size: Some(r.size()), ..Default::default() });
            }
            if active {
                draw_rectangle(r.x, r.y, 2., r.h, BORDER);
                draw_rectangle(r.x, r.y + 1., 2., r.h - 2., Color::from_rgba(185, 158, 75, 255));
            }
            let color = if target.is_some() { WHITE } else { Color::from_rgba(170, 170, 170, 255) };
            draw_texture(icon, r.x + 5. + ((72. - icon.width()) * 0.5).floor(),
                r.y + ((r.h - icon.height()) * 0.5).floor(), color);
            dialog::text(art.font.as_ref(), art.locale.ui(label), r.x + 87., r.y + 47., 17,
                if target.is_some() { LABEL } else { Color::from_rgba(128, 128, 128, 255) });
        }
    }

    fn check(&self, art: &Art, r: Rect, label: &str, checked: bool, focused: bool) {
        let tex = &self.checks[2 * usize::from(checked) + usize::from(focused)];
        draw_texture(tex, r.x, r.y, WHITE);
        dialog::text(art.font.as_ref(), art.locale.ui(label), r.x + 25., r.y + 16., 15, GOLD);
    }

    fn slider(&self, r: Rect, step: u8, max: u8, focus: bool, drag: bool) {
        let x = r.x + ((r.w - 19.) * f32::from(step) / f32::from(max)).round();
        let y = r.y + 11.;
        draw_rectangle(r.x, y, x - r.x, 1., Color::from_rgba(162, 127, 68, 255));
        draw_rectangle(x, y, r.right() - 3. - x, 1., BORDER);
        draw_texture(&self.sliders[if drag { 2 } else { usize::from(focus) }], x, r.y, WHITE);
    }
}

fn pointed_step(r: Rect, x: f32, max: u8) -> u8 {
    (((x - r.x - 8.) / (r.w - 19.)).clamp(0., 1.) * f32::from(max)).round() as u8
}

fn slider_value(control: Control, speed: Speed) -> Option<(u8, u8)> {
    match control {
        Control::Scroll => Some((speed.scroll - 1, 99)),
        Control::Factor => Some((speed.factor, (FACTORS.len() - 1) as u8)),
        _ => None,
    }
}

fn set_step(control: Control, speed: &mut Speed, step: u8) {
    match control {
        Control::Scroll => speed.scroll = step + 1,
        Control::Factor => speed.factor = step,
        _ => {}
    }
}

pub(super) async fn show(
    root: &Path, art: &Art, fullscreen: &mut bool, windowed_size: &mut Vec2,
    draw_behind: impl Fn(f32, f32),
) -> Result<(), Box<dyn Error>> {
    use Control::*;
    let skin = WidgetArt::load(root)?;
    let font = art.font.as_ref();
    let mut speed = Speed::load()?;
    let mut saved_speed = speed;
    let mut grid = display_preferences::grid()?;
    let mut page = Page::General;
    let mut focus = GeneralTab;
    let mut down = None;
    let mut error = None::<String>;
    loop {
        let scale = (screen_width() / 795.).min(screen_height() / 612.).clamp(0.01, 1.);
        let view = vec2(screen_width(), screen_height()) / scale;
        set_camera(&dialog::setup_camera(view.x, view.y));
        draw_behind(view.x, view.y);
        let scroll_label = art.locale.ui("Scroll speed:");
        let label_width = measure_text(scroll_label, font, 17, 1.).width.round();
        let layout = Layout::new(view, label_width);
        let controls = page.controls();
        let mouse = Vec2::from(mouse_position()) / scale;
        let hover = controls.iter().copied().find(|&c| layout.control(c).contains(mouse));
        if is_key_pressed(KeyCode::Tab) || is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::Down) {
            let back = is_key_pressed(KeyCode::Up) || is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            let index = controls.iter().position(|&c| c == focus).unwrap_or(0);
            focus = controls[(index + if back { controls.len() - 1 } else { 1 }) % controls.len()];
        }
        if is_mouse_button_pressed(MouseButton::Left) {
            down = hover;
            if let Some(c) = hover { focus = c; }
        }
        let released = is_mouse_button_released(MouseButton::Left);
        if let Some(c) = down && let Some((_, max)) = slider_value(c, speed) {
            set_step(c, &mut speed, pointed_step(layout.control(c), mouse.x, max));
        }
        if down.is_none() && let Some((step, max)) = slider_value(focus, speed) {
            if is_key_pressed(KeyCode::Left) { set_step(focus, &mut speed, step.saturating_sub(1)); }
            if is_key_pressed(KeyCode::Right) { set_step(focus, &mut speed, (step + 1).min(max)); }
            if is_key_pressed(KeyCode::Home) { set_step(focus, &mut speed, 0); }
            if is_key_pressed(KeyCode::End) { set_step(focus, &mut speed, max); }
        }
        let activate = if released { down.take().filter(|&c| Some(c) == hover) }
            else if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) { Some(focus) }
            else { None };
        if is_key_pressed(KeyCode::Escape) || activate == Some(Close) { return Ok(()); }
        let change: io::Result<()> = match activate {
            Some(GeneralTab) => { page = Page::General; Ok(()) }
            Some(DisplayTab) => { page = Page::Display; Ok(()) }
            Some(Accelerated) => { speed.accelerated = !speed.accelerated; Ok(()) }
            Some(Grid) => display_preferences::save_grid(!grid).map(|()| grid = !grid),
            Some(Fullscreen) => display_preferences::save(!*fullscreen).map(|()| {
                if !*fullscreen { *windowed_size = vec2(screen_width(), screen_height()); }
                *fullscreen = !*fullscreen;
                set_fullscreen(*fullscreen);
                if !*fullscreen { request_new_screen_size(windowed_size.x, windowed_size.y); }
            }),
            _ => Ok(()),
        };
        if let Err(e) = change { error = Some(e.to_string()); }
        if down.is_none() && speed != saved_speed {
            match speed.save() {
                Ok(()) => { saved_speed = speed; error = None; }
                Err(e) => { speed = saved_speed; error = Some(e.to_string()); }
            }
        }
        skin.frame(&layout);
        let window = layout.window;
        dialog::text(font, art.locale.ui("Preferences"), window.x + 15., window.y + 37., 22, GOLD);
        let highlight = hover.or(Some(focus));
        skin.tabs(art, &layout, page, highlight);
        match page {
            Page::General => {
                dialog::text(font, scroll_label, window.x + 217., window.y + 69., 17, LABEL);
                skin.check(art, layout.control(Accelerated), "Accelerated speed", speed.accelerated, highlight == Some(Accelerated));
                let caption = format!("{}  {}", art.locale.ui("Acceleration factor:"), FACTORS[usize::from(speed.factor)]);
                dialog::text(font, &caption, window.x + 242., window.y + 167., 15, LABEL);
                for control in [Scroll, Factor] {
                    let (step, max) = slider_value(control, speed).unwrap();
                    skin.slider(layout.control(control), step, max, highlight == Some(control), down == Some(control));
                }
            }
            Page::Display => {
                skin.check(art, layout.control(Fullscreen), "Full screen", *fullscreen, highlight == Some(Fullscreen));
                skin.check(art, layout.control(Grid), "Grid overlay", grid, highlight == Some(Grid));
            }
        }
        if let Some(message) = &error {
            let message = dialog::fit(font, message.clone(), window.w - 190., 15);
            dialog::text(font, &message, window.x + 53., window.bottom() - 24., 15, Color::from_rgba(255, 140, 135, 255));
        }
        art.about.draw_icon(layout.control(About), highlight == Some(About), down == Some(About) && hover == down);
        art.button(layout.control(Close), art.locale.ui("Close"), highlight == Some(Close), down == Some(Close) && hover == down);
        set_default_camera();
        next_frame().await;
        if activate == Some(About) {
            version::show(root, art).await?;
            next_frame().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_default_page_places_tabs_and_interactive_sliders() {
        let layout = Layout::new(vec2(1200., 820.), 94.);
        assert_eq!(layout.window, Rect::new(203., 105., 793., 610.));
        assert_eq!(layout.control(Control::DisplayTab), Rect::new(218., 321., 192., 82.));
        let scroll = layout.control(Control::Scroll);
        assert_eq!(scroll, Rect::new(524., 157., 454., 22.));
        assert_eq!(pointed_step(scroll, scroll.x, 99), 0);
        assert_eq!(pointed_step(scroll, scroll.right(), 99), 99);
        let factor = layout.control(Control::Factor);
        let mut speed = Speed::default();
        set_step(Control::Factor, &mut speed, pointed_step(factor, 780., 11));
        assert_eq!(speed.factor, 7);
        assert!(Page::General.controls().contains(&Control::Scroll));
        assert!(Page::Display.controls().contains(&Control::Fullscreen));
        assert!(!Page::Display.controls().contains(&Control::Factor));
    }
}

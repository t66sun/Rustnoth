use crate::{BoardCamera, BoardData, dialog, render};
use macroquad::prelude::*;
use rustnoth_gui::{save_replay::Event, session::Session};
use std::{error::Error, io, path::Path};

/// Replays have an incoming campaign roster, and always begin before their first command.
pub(super) async fn watch(session: &mut Session<'_>, events: &[Event], data: &BoardData,
    art: &mut render::BoardArt, camera: &mut BoardCamera) -> Result<(), Box<dyn Error>> {
    let load = |file: &str| data.assets.resolve_image(Path::new(file)).and_then(|p| render::load_texture(&p, None));
    let panel = load("themes/editor/classic/toolbar.png");
    let frame = load("buttons/button_square/button_square_30.png");
    let icons = ["play", "pause", "play_move"].map(|name| load(&format!("icons/action/{name}_30.png")));
    let mut cursor = 0;
    let mut playing = false;
    let mut next = 0.;
    let mut minimap_drag = render::minimap::Drag::default();
    if let Some(view) = session.observation() { art.load(view, &data.assets, session); }
    next_frame().await;
    loop {
        if is_key_pressed(KeyCode::Escape) { return Ok(()); }
        let point = Vec2::from(mouse_position());
        let mut viewport = render::board_viewport();
        let top = viewport.y;
        viewport.y += 41.;
        viewport.height -= 41.;
        let controls = [7., 41., 177.].map(|x| Rect::new(viewport.x + x, top + 5., 30., 30.));
        if let Some(view) = session.observation() {
            let fit = is_key_pressed(KeyCode::Home);
            camera.update(viewport, view, fit);
            let pressed = [MouseButton::Left, MouseButton::Middle]
                .into_iter().find(|&button| is_mouse_button_pressed(button));
            pan_minimap(camera, &mut minimap_drag, pressed, is_mouse_button_down, || {
                if fit || controls.iter().any(|bounds| bounds.contains(point)) { return None; }
                render::minimap_point(view, point)
            });
            camera.scroll([point.x,point.y], mouse_wheel().1);
            camera.drag([point.x,point.y], is_mouse_button_pressed(MouseButton::Middle), is_mouse_button_down(MouseButton::Middle));
        } else {
            minimap_drag.cancel();
        }
        let status = format!("Replay {cursor}/{}   ·   Space: play/pause   ·   Right: next move   ·   Esc: close",events.len());
        let mut step = is_key_pressed(KeyCode::Right);
        if is_key_pressed(KeyCode::Space) { playing = !playing; }
        render::draw(session.state(),session.observation(),camera.layout(),art,render::MovementView {
            session, global_time: session.global_time_of_day(), route: None, selected: None,
            actions: &[], message: Some(&status), assets: &data.assets, menu: None,
        }, |art| {
            if let Some(tex) = &panel {
                let mut x = viewport.x;
                while x < viewport.x + viewport.width {
                    let w = tex.width().min(viewport.x + viewport.width - x);
                    draw_texture_ex(tex, x, top, WHITE, DrawTextureParams { source: Some(Rect::new(0.,0.,w,41.)), ..Default::default() });
                    x += w;
                }
            }
            if let Some(line) = session.dialogue() { render::draw_dialogue(line, &data.assets, art, 0); }
            for (index, r) in controls.into_iter().enumerate() {
                if let Some(tex) = &frame { draw_texture(tex,r.x,r.y,WHITE); }
                if let Some(tex) = &icons[index] { draw_texture(tex,r.x,r.y,WHITE); }
                if r.contains(point) && is_mouse_button_pressed(MouseButton::Left) {
                    match index { 0 => playing = true, 1 => playing = false, _ => step = true }
                }
            }
        });
        if (step || playing && get_time() >= next) && let Some(event) = events.get(cursor) {
            event.apply(session)?;
            cursor += 1;
            next = get_time() + 0.65;
            if let Some(view) = session.observation() { art.load(view, &data.assets, session); }
        }
        if cursor == events.len() { playing = false; }
        next_frame().await;
    }
}

fn pan_minimap(
    camera: &mut BoardCamera,
    drag: &mut render::minimap::Drag,
    pressed: Option<MouseButton>,
    is_held: impl FnOnce(MouseButton) -> bool,
    world_point: impl FnOnce() -> Option<[f32; 2]>,
) {
    if let Some(point) = drag.update(pressed, is_held, world_point) {
        camera.center_world(point);
    }
}

pub(super) async fn save_error(root: &Path, error: &io::Error) {
    let skin = dialog::Skin::load(root);
    let backdrop = dialog::Backdrop::capture(FilterMode::Linear);
    next_frame().await;
    loop {
        let (r,mouse) = skin.frame("Could not save scenario replay", 530., 195., backdrop.texture());
        dialog::wrap(skin.font.as_ref(), &error.to_string(), r.x+18.,r.y+77.,r.w-36.);
        let done = skin.button(Rect::new(r.right()-125.,r.bottom()-46.,106.,30.), mouse, "Continue", true)
            || is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape);
        set_default_camera();
        next_frame().await;
        if done { break; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::observation::Observation;
    use rustnoth_gui::layout::Viewport;

    #[test]
    fn replay_centers_on_each_minimap_motion_before_the_viewport_is_drawn() {
        let observation = Observation {
            side: 1, allied_sides: vec![], gold: None, width: 40, height: 30,
            tiles: vec![], units: vec![],
        };
        let viewport = Viewport { x: 0.0, y: 68.0, width: 400.0, height: 300.0 };
        let mut camera = BoardCamera::default();
        camera.update(viewport, &observation, false);
        let original = camera.checkpoint().unwrap();
        let mut drag = render::minimap::Drag::default();
        let mut update = |pressed, point| {
            pan_minimap(&mut camera, &mut drag, pressed, |button| button == MouseButton::Middle, || point);
            camera.checkpoint().unwrap()
        };

        let first = update(Some(MouseButton::Middle), Some([8.5, 8.5]));
        let second = update(None, Some([16.5, 16.5]));
        assert_ne!(first, original);
        assert!(second[0] > first[0] && second[1] > first[1]);
        assert_eq!(second[2], original[2]);
        assert_eq!(update(None, None), second);
        assert_eq!(update(None, Some([8.5, 8.5])), second);
    }
}

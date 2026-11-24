use crate::{BoardCamera, BoardData, camera::PointerEvent, dialog, render};
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
        let view = session.observation();
        let fit = is_key_pressed(KeyCode::Home);
        if let Some(view) = view {
            camera.update(viewport, view, fit);
            camera.scroll([point.x,point.y], mouse_wheel().1);
        }
        let map_input = view.is_some() && !fit;
        if !map_input {
            minimap_drag.cancel();
            camera.cancel_drag();
        }
        let mut playback_button = None;
        camera.pointer_input(true, 1.0, |camera, event| {
            let handled = route_pointer(camera, &mut minimap_drag, &controls, &mut playback_button, event, |point| {
                let view = view.filter(|_| map_input)?;
                render::minimap_point(view, point)
            });
            handled || !map_input
        });
        minimap_drag.release_if_unheld(is_mouse_button_down);
        let status = format!("Replay {cursor}/{}   ·   Space: play/pause   ·   Right: next move   ·   Esc: close",events.len());
        let mut step = is_key_pressed(KeyCode::Right);
        if is_key_pressed(KeyCode::Space) { playing = !playing; }
        render::draw(session.state(),session.observation(),camera.layout(),art,render::MovementView {
            session, global_time: session.global_time_of_day(), route: None, selected: None,
            hold_map_hover: camera.dragging(),
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
            }
        });
        match playback_button {
            Some(0) => playing = true,
            Some(1) => playing = false,
            Some(2) => step = true,
            _ => {}
        }
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

fn route_pointer(
    camera: &mut BoardCamera,
    drag: &mut render::minimap::Drag,
    controls: &[Rect; 3],
    playback_button: &mut Option<usize>,
    event: PointerEvent,
    world_point: impl FnOnce(Vec2) -> Option<[f32; 2]>,
) -> bool {
    if let PointerEvent::Press(MouseButton::Left, point) = event
        && let Some(index) = controls.iter().position(|rect| rect.contains(Vec2::from_array(point)))
    {
        *playback_button = Some(index);
        drag.cancel();
        return true;
    }
    if camera.dragging() { return false; }
    drag.pointer_event(event, |point| {
        if controls.iter().any(|rect| rect.contains(point)) { return None; }
        world_point(point)
    }, |world| camera.center_world(world))
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
    fn replay_routes_motion_and_toolbar_actions_by_the_original_press() {
        let observation = Observation {
            side: 1, allied_sides: vec![], gold: None, width: 40, height: 30,
            tiles: vec![], units: vec![],
        };
        let viewport = Viewport { x: 0.0, y: 68.0, width: 400.0, height: 300.0 };
        let mut camera = BoardCamera::default();
        camera.update(viewport, &observation, false);
        let original = camera.checkpoint().unwrap();
        let mut drag = render::minimap::Drag::default();
        let controls = [7., 41., 177.].map(|x| Rect::new(x, 32.0, 30.0, 30.0));
        let mut button = None;
        let mut route = |event| {
            let handled = route_pointer(&mut camera, &mut drag, &controls, &mut button, event, |point| {
                (point.x > 400.0).then_some([point.x - 400.0, point.y])
            });
            (handled, button, camera.checkpoint().unwrap())
        };
        assert_eq!(route(PointerEvent::Press(MouseButton::Middle, [125.0, 140.0])), (false, None, original));
        assert_eq!(route(PointerEvent::Move([416.5, 16.5])), (false, None, original));

        let (handled, _, first) = route(PointerEvent::Press(MouseButton::Middle, [408.5, 8.5]));
        assert!(handled);
        let (_, _, moved) = route(PointerEvent::Move([416.5, 16.5]));
        let (_, _, released) = route(PointerEvent::Release(MouseButton::Middle, [420.5, 20.5]));
        assert!(moved[0] > first[0] && moved[1] > first[1]);
        assert!(released[0] > moved[0] && released[1] > moved[1]);
        assert_eq!(released[2], original[2]);
        assert_eq!(route(PointerEvent::Move([408.5, 8.5])), (false, None, released));
        assert_eq!(route(PointerEvent::Move([45.0, 45.0])), (false, None, released));

        assert_eq!(route(PointerEvent::Press(MouseButton::Left, [45.0, 45.0])), (true, Some(1), released));
        assert_eq!(route(PointerEvent::Move([408.5, 8.5])), (false, Some(1), released));
    }
}

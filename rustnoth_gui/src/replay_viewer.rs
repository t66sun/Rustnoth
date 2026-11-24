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
    if let Some(view) = session.observation() { art.load(view, &data.assets, session); }
    next_frame().await;
    loop {
        if is_key_pressed(KeyCode::Escape) { return Ok(()); }
        let point = Vec2::from(mouse_position());
        let mut viewport = render::board_viewport();
        let top = viewport.y;
        viewport.y += 41.;
        viewport.height -= 41.;
        if let Some(view) = session.observation() {
            camera.update(viewport, view, is_key_pressed(KeyCode::Home));
            camera.scroll([point.x,point.y], mouse_wheel().1);
            camera.drag_input(true, 1.0);
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
            for (index, x) in [7.,41.,177.].into_iter().enumerate() {
                let r = Rect::new(viewport.x+x, top+5.,30.,30.);
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

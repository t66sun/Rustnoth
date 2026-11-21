use super::{Art, AssetCatalog, EditableMap};
use crate::{dialog, render, title_screen::button_canvas::{self, State}};
use macroquad::prelude::*;
use std::{error::Error, path::Path};

pub(super) fn make(width: usize, height: usize, terrain: &str) -> Result<EditableMap, Box<dyn Error>> {
    let row = vec![terrain; width + 2].join(", ");
    Ok(EditableMap::parse(vec![row; height + 2].join("\n") + "\n")?)
}

fn pointed(mouse: f32, left: f32, width: f32) -> i32 {
    (1. + (mouse-left) / width * 199.).round().clamp(1., 200.) as i32
}

pub(super) async fn show(root: &Path, assets: &AssetCatalog, art: &mut Art,
        font: Option<&Font>, map: &EditableMap) -> Result<Option<[usize;2]>, Box<dyn Error>> {
    let background = Texture2D::from_image(&get_screen_data());
    let mut textures = Vec::new();
    for suffix in ["", "-active", "-pressed"] {
        let path = root.join(format!("images/buttons/button_normal/background{suffix}.png"));
        let tex = render::load_texture(&path, None).ok_or("Missing GUI2 button image")?;
        tex.set_filter(FilterMode::Nearest);
        textures.push(tex);
    }
    let mut values = [map.map().width().clamp(1,200), map.map().height().clamp(1,200)];
    let mut focus = 0usize;
    let mut held = None;
    next_frame().await;
    loop {
        let scale = (screen_width()/390.).min(screen_height()/220.).clamp(0.01,1.);
        let (w,h) = (screen_width()/scale,screen_height()/scale);
        let panel = Rect::new(((w-370.)/2.).floor(),((h-191.)/2.).floor(),370.,191.);
        let (mx,my) = mouse_position();
        let pointer = vec2(mx/scale,my/scale);
        let down = is_mouse_button_pressed(MouseButton::Left);
        let up = is_mouse_button_released(MouseButton::Left);
        set_camera(&dialog::setup_camera(w,h));
        draw_texture_ex(&background,0.,0.,WHITE,DrawTextureParams { dest_size:Some(vec2(w,h)),..Default::default() });
        draw_rectangle(panel.x,panel.y,panel.w,panel.h,Color::from_rgba(114,79,46,255));
        art.panel(assets,"dialogs/opaque-background.png",Rect::new(panel.x+2.,panel.y+2.,panel.w-4.,panel.h-4.));
        dialog::text(font,"New Map",panel.x+15.,panel.y+35.,22,dialog::GOLD);
        if is_key_pressed(KeyCode::Tab) {
            let back = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            focus = (focus + if back {3} else {1}) % 4;
        }
        if focus < 2 {
            let delta = i32::from(is_key_pressed(KeyCode::Right)) - i32::from(is_key_pressed(KeyCode::Left));
            values[focus] = (values[focus]+delta).clamp(1,200);
        }
        for (i,label) in ["Width:", "Height:"].into_iter().enumerate() {
            let y = panel.y+53.+(i as f32)*38.;
            let rail = Rect::new(panel.x+83., y, 250., 22.);
            if down && rail.contains(pointer) { focus=i; held=Some(i); }
            if held == Some(i) { values[i] = pointed(pointer.x,rail.x+6.,rail.w-12.); }
            let x = rail.x+6.+ ((values[i]-1) as f32*(rail.w-12.)/199.).round();
            dialog::text(font,label,panel.x+15.,y+17.,17,WHITE);
            draw_rectangle(rail.x+5.,y+10.,x-rail.x-5.,2.,dialog::GOLD);
            draw_rectangle(x,y+10.,rail.right()-5.-x,2.,Color::from_rgba(84,69,55,255));
            let state = if held==Some(i) {"-pressed"} else if rail.contains(pointer) || focus==i {"-active"} else {""};
            art.image(assets,&format!("buttons/sliders/slider{state}.png"),Rect::new(x-6.,y,12.,22.));
            dialog::text(font,&values[i].to_string(),rail.right()+2.,y+17.,14,WHITE);
        }
        let mut response = None;
        for (i,label) in ["Create","Cancel"].into_iter().enumerate() {
            let index = 2+i;
            let rect = Rect::new(panel.right()-248.+120.*i as f32,panel.bottom()-45.,110.,30.);
            let hot=rect.contains(pointer);
            if down && hot { focus=index; held=Some(index); }
            let (state,tex)=if held==Some(index) && hot {(State::Pressed,2)} else if hot || focus==index {(State::Focused,1)} else {(State::Enabled,0)};
            button_canvas::draw(rect,&textures[tex],state);
            let metrics=measure_text(label,font,16,1.);
            dialog::text(font,label,rect.center().x-metrics.width/2.,rect.center().y+metrics.height/2.,16,dialog::GOLD);
            if up && held==Some(index) && hot { response=Some(i==0); }
        }
        if up { held=None; }
        if is_key_pressed(KeyCode::Escape) { response=Some(false); }
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) { response=Some(focus!=3); }
        set_default_camera();
        if let Some(create)=response { return Ok(create.then(|| values.map(|v| v as usize))); }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::hex::Hex;
    #[test]
    fn new_maps_fill_the_playable_hexes_and_border_with_the_background_terrain() {
        let map = make(2,1,"Ww").unwrap();
        assert_eq!((map.map().width(),map.map().height()),(2,1));
        for y in -1..=1 { for x in -1..=2 { assert_eq!(map.map().terrain_at(Hex{x,y}),Some("Ww")); } }
        assert_eq!((pointed(-5.,10.,250.),pointed(500.,10.,250.)),(1,200));
    }
}

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
    Ok(dimensions(root, assets, art, font, map, false).await?.map(|choice| choice.size))
}

pub(super) struct Dimensions {
    pub size: [usize; 2],
    pub anchor: usize,
    pub copy_edges: bool,
}

fn direction_icon(index: usize, anchor: usize, diff: [i32; 2]) -> &'static str {
    let x = (index % 3) as i32 - (anchor % 3) as i32;
    let y = (index / 3) as i32 - (anchor / 3) as i32;
    let [dw, dh] = diff;
    if x == 0 && y == 0 { return "center"; }
    if x.abs() > 1 || y.abs() > 1 { return "none"; }
    if y == 0 { return match x * dw.signum() { -1 => "left", 1 => "right", _ => "none" }; }
    if x == 0 { return match y * dh.signum() { -1 => "up", 1 => "down", _ => "none" }; }
    let reverse = dw < 0 || dh < 0;
    if !reverse && (dw == 0 || dh == 0) { return "none"; }
    let (x,y) = if reverse { (-x,-y) } else { (x,y) };
    match (x,y) { (-1,-1) => "topleft", (1,-1) => "topright", (-1,1) => "bottomleft", _ => "bottomright" }
}

pub(super) async fn dimensions(root: &Path, assets: &AssetCatalog, art: &mut Art,
        font: Option<&Font>, map: &EditableMap, resize: bool) -> Result<Option<Dimensions>, Box<dyn Error>> {
    let background = Texture2D::from_image(&get_screen_data());
    let mut textures = Vec::new();
    for suffix in ["", "-active", "-pressed"] {
        let path = root.join(format!("images/buttons/button_normal/background{suffix}.png"));
        let tex = render::load_texture(&path, None).ok_or("Missing GUI2 button image")?;
        tex.set_filter(FilterMode::Nearest);
        textures.push(tex);
    }
    let old = [map.map().width(), map.map().height()];
    let mut values = old.map(|n| n.clamp(1, 200));
    let mut anchor = 0;
    let mut copy_edges = false;
    let (pw, ph) = if resize { (575., 341.) } else { (370., 191.) };
    let controls = if resize { 14 } else { 4 };
    let mut focus = 0usize;
    let mut held = None;
    next_frame().await;
    loop {
        let scale = (screen_width()/(pw+20.)).min(screen_height()/(ph+29.)).clamp(0.01,1.);
        let (w,h) = (screen_width()/scale,screen_height()/scale);
        let panel = Rect::new(((w-pw)/2.).floor(),((h-ph)/2.).floor(),pw,ph);
        let (mx,my) = mouse_position();
        let pointer = vec2(mx/scale,my/scale);
        let down = is_mouse_button_pressed(MouseButton::Left);
        let up = is_mouse_button_released(MouseButton::Left);
        set_camera(&dialog::setup_camera(w,h));
        draw_texture_ex(&background,0.,0.,WHITE,DrawTextureParams { dest_size:Some(vec2(w,h)),..Default::default() });
        draw_rectangle(panel.x,panel.y,panel.w,panel.h,Color::from_rgba(114,79,46,255));
        art.panel(assets,"dialogs/opaque-background.png",Rect::new(panel.x+2.,panel.y+2.,panel.w-4.,panel.h-4.));
        dialog::text(font,if resize { "Resize Map" } else { "New Map" },panel.x+15.,panel.y+35.,22,dialog::GOLD);
        if is_key_pressed(KeyCode::Tab) {
            let back = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            focus = (focus + if back {controls-1} else {1}) % controls;
        }
        if focus < 2 {
            let delta = i32::from(is_key_pressed(KeyCode::Right)) - i32::from(is_key_pressed(KeyCode::Left));
            values[focus] = (values[focus]+delta).clamp(1,200);
        }
        let names = if resize { ["New width:", "New height:"] } else { ["Width:", "Height:"] };
        for (i,label) in names.into_iter().enumerate() {
            let y = panel.y+53.+(i as f32)*38.;
            let rail = Rect::new(panel.x + if resize {115.} else {83.}, y, 250., 22.);
            if resize {
                let label = if i == 0 { "Old width:" } else { "Old height:" };
                dialog::text(font,&format!("{label}   {}", old[i]),panel.x+410.,y+17.,17,WHITE);
            }
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
        if resize {
            dialog::text(font, "Expand/shrink direction:", panel.x+15.,panel.y+155.,17,WHITE);
            let diff = [values[0] - old[0], values[1] - old[1]];
            for i in 0..9 {
                let area = Rect::new(panel.x+215.+32.*(i%3) as f32,panel.y+135.+32.*(i/3) as f32,32.,32.);
                if down && area.contains(pointer) { focus=i+2; held=Some(focus); }
                if (up && held==Some(i+2) && area.contains(pointer)) || (focus==i+2 && is_key_pressed(KeyCode::Space)) {
                    anchor=i;
                }
                let state = if i==anchor { "-pressed" } else if area.contains(pointer) || focus==i+2 { "-active" } else { "" };
                art.image(assets, &format!("buttons/button_square/button_square_30{state}.png"), area);
                let icon = direction_icon(i,anchor,diff);
                art.image(assets,&format!("icons/arrows/arrows_blank_{icon}_30.png"),Rect::new(area.x+1.,area.y+1.,30.,30.));
            }
            let box_rect = Rect::new(panel.x+15.,panel.y+252.,19.,19.);
            let hit = Rect::new(box_rect.x,box_rect.y,185.,box_rect.h);
            if down && hit.contains(pointer) { focus=11; held=Some(focus); }
            if (up && held==Some(11) && hit.contains(pointer)) || (focus==11 && is_key_pressed(KeyCode::Space)) {
                copy_edges=!copy_edges;
            }
            let state = match (copy_edges, hit.contains(pointer) || focus==11) {
                (true,true) => "-active-pressed", (true,false) => "-pressed", (false,true) => "-active", _ => "",
            };
            art.image(assets,&format!("buttons/checkbox{state}.png"),box_rect);
            dialog::text(font,"Copy edge terrain",box_rect.right()+5.,box_rect.y+16.,17,WHITE);
        }
        let mut response = None;
        for (i,label) in [if resize {"Resize"} else {"Create"},"Cancel"].into_iter().enumerate() {
            let index = controls-2+i;
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
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) { response=Some(focus!=controls-1); }
        set_default_camera();
        if let Some(create)=response { return Ok(create.then(|| Dimensions { size:values.map(|v| v as usize), anchor, copy_edges })); }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::hex::Hex;
    #[test]
    fn resize_grid_reverses_shrink_arrows_around_the_selected_anchor() {
        assert_eq!(direction_icon(4,4,[2,2]),"center");
        assert_eq!(direction_icon(0,4,[2,2]),"topleft");
        assert_eq!(direction_icon(0,4,[-2,2]),"bottomright");
        assert_eq!(direction_icon(7,4,[0,-2]),"up");
        assert_eq!(direction_icon(2,0,[3,0]),"none");
    }
    #[test]
    fn new_maps_fill_the_playable_hexes_and_border_with_the_background_terrain() {
        let map = make(2,1,"Ww").unwrap();
        assert_eq!((map.map().width(),map.map().height()),(2,1));
        for y in -1..=1 { for x in -1..=2 { assert_eq!(map.map().terrain_at(Hex{x,y}),Some("Ww")); } }
        assert_eq!((pointed(-5.,10.,250.),pointed(500.,10.,250.)),(1,200));
    }
}

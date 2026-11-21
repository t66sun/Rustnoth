//! The stock tutorial opens on Delfador before it places the chosen student.
use super::{Hero, Node, opening::children, put};
use crate::{BoardData, camera::BoardCamera, desktop, dialog, display_preferences::speed::Speed, map_menus, render, title_screen};
use engine_lite::{hex::Hex, map::ScenarioMap, wml_emit};
use macroquad::prelude::*;
use rustnoth_gui::{session::{Players, Session}, team_colors::TeamColors};
use std::{error::Error, fs, path::Path};

fn preview_scenario(level: &mut Node) -> Result<Hex, Box<dyn Error>> {
    let start = children(level, "event").find(|e| e.attribute("name") == Some("start"))
        .ok_or("missing tutorial start")?;
    let pick = start.children.iter().position(|(tag, _)| tag == "select_character")
        .ok_or("missing tutorial character selection")?;
    let introduction = start.children[..pick].iter()
        .find(|(tag, cmd)| tag == "message" && cmd.attribute("speaker") == Some("narrator"))
        .ok_or("missing introduction before character selection")?.1.clone();
    let keep = ScenarioMap::parse(level.attribute("map_data").ok_or("missing tutorial map")?)?
        .special_location("1").ok_or("missing student keep")?;
    let student = level.children.iter_mut().find(|(tag, side)| tag == "side" && side.attribute("id") == Some("student"))
        .ok_or("missing tutorial student side")?;
    // The stock prestart uses store_unit with kill=yes. The student only returns
    // to the map after the character dialog closes.
    put(&mut student.1, "no_leader", "yes");
    put(&mut student.1, "defeat_condition", "never");
    level.children.retain(|(tag, _)| tag != "event");
    let mut start = Node::default();
    put(&mut start, "name", "start");
    start.children.push(("message".into(), introduction));
    level.children.push(("event".into(), start));
    Ok(keep)
}

struct Picker {
    sprites: [Texture2D; 2],
    panel: Option<Texture2D>,
    buttons: [Option<Texture2D>; 2],
    title: String,
    question: String,
    end_turn: String,
    names: [String; 2],
    focus: usize,
    pressed: Option<usize>,
    pointer: (f32, f32),
}

impl Picker {
    fn load(root: &Path) -> Result<Self, Box<dyn Error>> {
        let folder = root.join(desktop::TUTORIAL);
        let colors = TeamColors::parse(&fs::read(root.join("data/core/team-colors.cfg"))?)?;
        let document = engine_lite::wml::parse(fs::read(folder.join("gui/character_selection.cfg"))?)?;
        fn widgets<'a>(cfg: &'a Node, name: &str, result: &mut Vec<&'a Node>) {
            for (tag, child) in &cfg.children {
                if tag == name { result.push(child); } else { widgets(child, name, result); }
            }
        }
        let mut labels = Vec::new();
        let mut buttons = Vec::new();
        widgets(&document, "label", &mut labels);
        widgets(&document, "button", &mut buttons);
        let [title, question] = labels[..] else { return Err("invalid tutorial character labels".into()) };
        let [male, female] = buttons[..] else { return Err("invalid tutorial character buttons".into()) };
        let dir = crate::display_preferences::config_directory()?;
        let mut locale = title_screen::locale::Locale::load(root, title_screen::locale::saved(&dir)?)?;
        let native = |s: &str| render::load_texture(&root.join("images").join(s), None);
        Ok(Self {
            sprites: [
                render::load_texture(&folder.join("images/units/konrad-fighter.png"), None).ok_or("missing Konrad sprite")?,
                render::load_texture(&folder.join("images/units/human-princess.png"), Some((&colors, 1))).ok_or("missing Li’sar sprite")?,
            ],
            panel: native("dialogs/translucent65-background.png"),
            buttons: [native("buttons/large-button.png"), native("buttons/large-button-active.png")],
            title: locale.attribute(root, title, "label", "Select Character"),
            question: locale.attribute(root, question, "label", "Who do you want to play as?"),
            end_turn: locale.game("End Turn").into(),
            names: [locale.attribute(root, male, "label", Hero::Konrad.name()), locale.attribute(root, female, "label", Hero::Lisar.name())],
            focus: 0, pressed: None, pointer: mouse_position(),
        })
    }

    fn draw(&mut self, font: Option<&Font>) -> Option<Hero> {
        let scale = (screen_width() / 268.).min(screen_height() / 207.).clamp(0.01, 1.);
        let view = vec2(screen_width(), screen_height());
        let panel = Rect::new(((view.x - 268.*scale)*0.5).floor(), ((view.y - 207.*scale)*0.5).floor(), 268.*scale, 207.*scale);
        let pos = |x, y| panel.point() + vec2(x, y) * scale;
        draw_rectangle(panel.x, panel.y, panel.w, panel.h, Color::from_rgba(12, 16, 24, 215));
        if let Some(tex) = &self.panel { title_screen::tiled(panel, tex); }
        draw_rectangle_lines(panel.x + 1., panel.y + 1., panel.w - 2., panel.h - 2., 2., Color::from_rgba(114,79,46,255));
        let size = |s: f32| (s*scale).round().max(1.) as u16;
        let title = pos(15., 37.);
        dialog::text(font, &self.title, title.x, title.y, size(22.), dialog::GOLD);
        let question = pos(15., 70.);
        dialog::text(font, &self.question, question.x, question.y, size(17.), dialog::PALE);
        let buttons = [13., 139.].map(|x| {let at = pos(x, 161.); Rect::new(at.x,at.y,116.*scale,30.*scale)});
        let mouse = mouse_position();
        let hover = buttons.iter().position(|r| r.contains(Vec2::from(mouse)));
        if mouse != self.pointer {
            if let Some(index) = hover { self.focus = index; }
            self.pointer = mouse;
        }
        if [KeyCode::Left, KeyCode::Right, KeyCode::Tab].into_iter().any(is_key_pressed) { self.focus = 1 - self.focus; }
        if is_mouse_button_pressed(MouseButton::Left) { self.pressed = hover; }
        for (i, r) in buttons.into_iter().enumerate() {
            let sprite = &self.sprites[i];
            let size = sprite.size() * scale;
            draw_texture_ex(sprite, r.center().x - size.x / 2., r.y - size.y - 5. * scale, WHITE,
                DrawTextureParams { dest_size: Some(size), ..Default::default() });
            dialog::Skin::paint(&self.buttons[usize::from(hover == Some(i) || self.focus == i)], r);
            let button_font_size = (17. * scale).round().max(1.) as u16;
            let text_width = measure_text(&self.names[i], font, button_font_size, 1.).width;
            dialog::text(font, &self.names[i], r.center().x - text_width / 2., r.center().y + 6. * scale,
                button_font_size, dialog::PALE);
        }
        let picked = if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) { Some(self.focus) }
            else if is_mouse_button_released(MouseButton::Left) { self.pressed.take().filter(|i| Some(*i)==hover) }
            else { None };
        picked.map(|i| [Hero::Konrad, Hero::Lisar][i])
    }
}

pub(crate) async fn choose(document: &Node, core: &[u8], maps: &Path, root: &Path, seed: u32) -> Result<Option<Hero>, Box<dyn Error>> {
    let mut preview = document.clone();
    let (_, level) = preview.children.iter_mut().find(|(tag, _)| tag == "scenario").ok_or("missing tutorial scenario")?;
    let keep = preview_scenario(level)?;
    let mut bytes = Vec::new();
    wml_emit::write_config(&mut bytes, &preview)?;
    let data = BoardData::from_source(&bytes, core, maps.into(), root.into(), seed)?;
    let game = data.scenario.start(seed, |name| fs::read_to_string(maps.join(name)))?;
    let session = Session::with_players(game, Players::Campaign);
    let view = session.observation().ok_or("missing tutorial opening view")?;
    let opening = session.dialogue().ok_or("missing authored tutorial introduction")?;
    let mut art = render::BoardArt::new(root, Speed::load()?)?;
    art.load(view, &data.assets, &session);
    let mut camera = BoardCamera::default();
    camera.update(render::board_viewport(), view, false);
    camera.open_at(Some(keep));
    let menus = map_menus::State::default();
    let menu_art = map_menus::Art::new(root);
    let mut picker = Picker::load(root)?;
    let mut introduction = true;
    let mut page = 0;
    loop {
        camera.update(render::board_viewport(), view, false);
        let (last, choice) = render::draw(session.state(), Some(view), camera.layout(), &mut art, render::MovementView {
            session: &session, global_time: session.global_time_of_day(), route: None, selected: None,
            actions: &[], message: None, assets: &data.assets, menu: None,
        }, |art| {
            menus.draw(&menu_art, art.font.as_ref(), &[]);
            if let Some(r) = render::end_turn_button(screen_width(), screen_height(), true) {
                render::draw_turn_button(r, false, false, art, &picker.end_turn);
            }
            if introduction { (render::draw_dialogue(opening, &data.assets, art, page), None) }
            else { (false, picker.draw(art.font.as_ref())) }
        });
        let cancel = !introduction && is_key_pressed(KeyCode::Escape);
        let advance = introduction && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space)
            || is_key_pressed(KeyCode::Escape) || is_mouse_button_released(MouseButton::Left));
        next_frame().await;
        if cancel { return Ok(None); }
        if choice.is_some() { return Ok(choice); }
        if advance { if last { introduction = false; } else { page += 1; } }
    }
}

#[test]
fn the_introduction_precedes_the_character_choice_and_keeps_the_mentor_on_the_board() {
    let mut level = engine_lite::wml::parse("map_data=\"Xv,Xv,Xv,Xv\nXv,1 Kh,Gg,Xv\nXv,Xv,Xv,Xv\"\n[side]\nid=student\ntype=Fighter\n[/side]\n[side]\n[unit]\nid=Delfador\n[/unit]\n[/side]\n[event]\nname=start\n[message]\nspeaker=narrator\nmessage=Before choosing\n[/message]\n[select_character]\n[/select_character]\n[message]\nspeaker=narrator\nmessage=After choosing\n[/message]\n[/event]").unwrap();
    assert_eq!(preview_scenario(&mut level).unwrap(), Hex::from_wml(1,1).unwrap());
    assert_eq!(children(&level,"side").next().unwrap().attribute("no_leader"),Some("yes"));
    assert_eq!(children(&level,"side").flat_map(|s| children(s,"unit")).next().unwrap().attribute("id"),Some("Delfador"));
    let start=children(&level,"event").next().unwrap();
    assert_eq!(start.children.len(),1);
    assert_eq!(start.children[0].1.attribute("message"),Some("Before choosing"));
}

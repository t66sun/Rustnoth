//! Campaign selection using the artwork and two-pane layout of campaign_dialog.cfg.
#[path = "campaign_difficulty.rs"]
mod difficulty;
use std::{collections::HashMap, fs, path::{Component, Path, PathBuf}};

use engine_lite::wml::Node;
use macroquad::prelude::*;
use crate::dialog::{fit, setup_camera, text, ListAction};
use crate::title_screen::locale::{self, Locale};

const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
const LABEL: Color = Color::from_rgba(221, 221, 221, 255);
const EDGE: Color = Color::from_rgba(114, 79, 46, 255);

struct Entry<'a> {
    name: String,
    description: String,
    icon: Option<&'a str>,
    image: Option<&'a str>,
    background: Option<&'a str>,
    scenario: usize,
    descriptor: Option<&'a Node>,
}

fn plain(source: &str) -> String {
    let mut tag = false;
    source.chars().filter(|ch| match ch {
        '<' => { tag = true; false }
        '>' => { tag = false; false }
        _ => !tag,
    }).collect::<String>().replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
}

fn entries<'a>(root: &'a Node, scenarios: &[(&'a str, &'a str)], locale: &mut Locale, install: &Path) -> (Vec<Entry<'a>>, Vec<Entry<'a>>) {
    let campaigns = root.children.iter().filter(|(tag, _)| tag == "campaign").filter_map(|(_, node)| {
        let id = node.attribute("first_scenario")?;
        let scenario = scenarios.iter().position(|(key, _)| *key == id)?;
        Some(Entry {
            name: locale.attribute(install, node, "name", id),
            description: plain(&locale.attribute(install, node, "description", "")),
            icon: node.attribute("icon"), image: node.attribute("image"),
            background: node.attribute("background"), scenario, descriptor: Some(node),
        })
    }).collect();
    let levels = root.children.iter().filter(|(tag, node)| tag == "scenario" && node.attribute("id").is_some())
        .zip(scenarios).enumerate().map(|(scenario, ((_, node), &(id, _)))| {
            // Scenario bundles need not include a [campaign] descriptor or a description.
            Entry {
                name: locale.attribute(install, node, "name", id),
                description: plain(&locale.attribute(install, node, "description", id)),
                icon: None, image: None, background: None, scenario, descriptor: None,
            }
        }).collect();
    (campaigns, levels)
}

fn filtered(entries: &[Entry<'_>], filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    entries.iter().enumerate().filter(|(_, entry)| entry.name.to_lowercase().contains(&needle))
        .map(|(i, _)| i).collect()
}

pub(super) struct Selector<'a> {
    campaigns: Vec<Entry<'a>>,
    levels: Vec<Entry<'a>>,
    pub(super) campaigns_only: bool,
    show_levels: bool,
    cursor: usize,
    first: usize,
    query: String,
    editing: bool,
    description_offset: usize,
    last_click: Option<(usize, f64)>,
    art: Art,
    difficulty_menu: difficulty::Menu<'a>,
}

impl<'a> Selector<'a> {
    pub fn new(root: &Path, campaign: &'a Node, scenarios: &[(&'a str, &'a str)]) -> std::io::Result<Self> {
        let selected = crate::display_preferences::config_directory().and_then(|dir| locale::saved(&dir)).unwrap_or_default();
        let mut language = Locale::load(root, selected)?;
        let (campaigns, levels) = entries(campaign, scenarios, &mut language, root);
        Ok(Self {
            campaigns_only: false, show_levels: campaigns.is_empty(), campaigns, levels, cursor: 0, first: 0,
            query: String::new(), editing: false, description_offset: 0, last_click: None,
            art: Art::new(root, campaign, language),
            difficulty_menu: difficulty::Menu::default(),
        })
    }

    pub(super) async fn difficulty(&mut self, campaign: &Node) -> Option<String> {
        if let Some(define) = self.difficulty_menu.define_for(campaign) {
            return Some(define.to_owned());
        }
        difficulty::choose(&mut self.art, campaign).await
    }

    pub fn draw(&mut self, error: Option<&str>) -> ListAction {
        let scale = (screen_width() / 760.0).min(screen_height() / 460.0).clamp(0.01, 1.0);
        let w = screen_width() / scale;
        let h = screen_height() / scale;
        set_camera(&setup_camera(w, h));
        let bounds = Rect::new(0.0, 0.0, w, h);
        clear_background(BLACK);
        if let Some(map) = self.art.get("maps/titlescreen.webp") { contain(&map, bounds, true); }
        let width = (w - 24.0).min(1050.0);
        let height = (h - 20.0).min(750.0);
        let window = Rect::new((w - width)*0.5, (h - height)*0.5, width, height);
        let panel = Rect::new(window.x+12.0, window.y+61.0, width-24.0, height-129.0);
        let list = Rect::new(panel.x+8.0, panel.y+8.0, (panel.w-26.0)*0.47, panel.h-16.0);
        let details = Rect::new(list.right()+12.0, list.y, panel.right()-list.right()-20.0, list.h);
        let search = Rect::new(panel.right()-212.0, window.y+14.0, 212.0, 32.0);
        let bottom = window.bottom()-53.0;
        let cancel = Rect::new(panel.right()-100.0, bottom, 100.0, 38.0);
        let play = Rect::new(cancel.x-148.0, bottom-2.0, 138.0, 42.0);
        let toggle = Rect::new(panel.x, bottom, 120.0, 38.0);
        let (mx, my) = mouse_position();
        let pointer = vec2(mx/scale, my/scale);
        let modal = self.difficulty_menu.is_open();
        let key = |code| !modal && is_key_pressed(code);
        let click = !modal && is_mouse_button_pressed(MouseButton::Left);
        if key(KeyCode::Escape) || (click && cancel.contains(pointer)) { return ListAction::Back; }
        if !self.campaigns_only && !self.campaigns.is_empty() && (click && toggle.contains(pointer) || key(KeyCode::Tab)) {
            self.show_levels = !self.show_levels;
            self.editing = false;
            self.query.clear();
            self.cursor = 0;
            self.first = 0;
            self.description_offset = 0;
            self.last_click = None;
        }
        if click { self.editing = search.contains(pointer); }
        if key(KeyCode::F) && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)) {
            self.editing = true;
        }
        if self.editing && !modal {
            let len = self.query.len();
            while let Some(c) = get_char_pressed() { if !c.is_control() { self.query.push(c); } }
            if is_key_pressed(KeyCode::Backspace) { self.query.pop(); }
            if self.query.len() != len {
                self.cursor = 0; self.first = 0; self.description_offset = 0; self.last_click = None;
            }
        }
        let entries = if self.show_levels { &self.levels } else { &self.campaigns };
        let rows = filtered(entries, &self.query);
        let row_h = if self.show_levels { 44.0 } else { 76.0 };
        let top = list.y+32.0;
        let visible = ((list.bottom()-top)/row_h).floor().max(1.0) as usize;
        let last = rows.len().saturating_sub(1);
        let previous = self.cursor;
        let step = if key(KeyCode::PageDown) { visible as isize }
            else if key(KeyCode::PageUp) { -(visible as isize) }
            else if key(KeyCode::Down) {1} else if key(KeyCode::Up) {-1} else {0};
        self.cursor = self.cursor.saturating_add_signed(step).min(last);
        if key(KeyCode::Home) { self.cursor = 0; }
        if key(KeyCode::End) { self.cursor = last; }
        let wheel = if modal { 0.0 } else { mouse_wheel().1 };
        if list.contains(pointer) && wheel != 0.0 {
            self.first = self.first.saturating_add_signed(if wheel > 0.0 {-3} else {3}).min(rows.len().saturating_sub(visible));
            self.cursor = self.cursor.clamp(self.first, (self.first+visible-1).min(last));
        }
        self.first = self.first.clamp(self.cursor.saturating_sub(visible-1), self.cursor)
            .min(rows.len().saturating_sub(visible));
        let row_click = click && list.contains(pointer) && pointer.y >= top;
        let mut confirmed = key(KeyCode::Enter) || (!self.editing && key(KeyCode::Space))
            || click && play.contains(pointer);
        if row_click {
            let row = ((pointer.y-top)/row_h) as usize;
            let index = self.first+row;
            if row < visible && index < rows.len() {
                self.cursor = index;
                let now = get_time();
                confirmed |= self.show_levels || self.last_click.is_some_and(|(other, time)| other == index && now-time < 0.4);
                self.last_click = Some((index, now));
            }
        }
        if self.cursor != previous { self.description_offset = 0; }
        let entry = rows.get(self.cursor).map(|&index| &entries[index]);
        let action = if confirmed && let Some(entry) = entry {
            self.last_click = None;
            ListAction::Choose(entry.scenario)
        } else {
            ListAction::Idle
        };

        self.art.window(window);
        self.art.label(self.art.locale.ui("Play a Campaign"), panel.x, window.y+40.0, 28, GOLD);
        fill(search, Color::from_rgba(10, 14, 21, 255));
        outline(search, if self.editing { GOLD } else { EDGE });
        let query_label = fit(self.art.font.as_ref(), self.query.clone(), search.w-16.0, 16);
        self.art.label(if self.query.is_empty() { self.art.locale.ui("Filter") } else { &query_label },
            search.x+8.0, search.y+22.0, 16, LABEL);
        fill(panel, BLACK);
        let backdrop = entry.and_then(|e| e.background).unwrap_or("story/landscape-battlefield_nohumans.webp");
        if let Some(tex) = self.art.get(backdrop) {
            let aspect = panel.w / panel.h;
            let size = vec2(tex.width().min(tex.height()*aspect), tex.height().min(tex.width()/aspect));
            let source = Rect::new((tex.width()-size.x)*0.5, (tex.height()-size.y)*0.5, size.x, size.y);
            draw_texture_ex(&tex, panel.x, panel.y, Color::new(1.0,1.0,1.0,0.25),
                DrawTextureParams { dest_size: Some(vec2(panel.w,panel.h)), source: Some(source), ..Default::default() });
        }
        outline(panel, EDGE);
        fill(list, Color::from_rgba(13, 17, 22, 142));
        fill(Rect::new(list.x, list.y, list.w, 30.0), Color::from_rgba(33, 43, 56, 210));
        self.art.label(self.art.locale.ui("Name"), list.x+10.0, list.y+21.0, 16, GOLD);
        for (row, &index) in rows.iter().skip(self.first).take(visible).enumerate() {
            let rect = Rect::new(list.x, top+row as f32*row_h, list.w, row_h-1.0);
            let item = &entries[index];
            if self.cursor == self.first+row || rect.contains(pointer) {
                fill(rect, Color::from_rgba(32,44,57,230));
                if let Some(texture) = self.art.get("dialogs/selection-background-fancy.png") {
                    image(&texture, rect);
                }
                if self.cursor == self.first+row { outline(rect, EDGE); }
            }
            let mut label_x = rect.x+12.0;
            if !self.show_levels {
                let cell = Rect::new(rect.x+5.0, rect.y+3.0, 70.0, 70.0);
                if let Some(icon) = item.icon.and_then(|id| self.art.get(id)) { contain(&icon, cell, false); }
                label_x = cell.right()+5.0;
            }
            let name = fit(self.art.font.as_ref(), item.name.clone(), rect.right()-label_x-4.0, 17);
            self.art.label(&name, label_x, rect.center().y+6.0, 17, LABEL);
        }
        if rows.is_empty() { self.art.label(self.art.locale.ui("No matches"), list.x+12.0, top+28.0, 16, LABEL); }
        if let Some(e) = entry {
            let font = self.art.font.as_ref();
            let name = fit(font, e.name.clone(), details.w-12.0, 21);
            text(font, &name, details.x+6.0, details.y+26.0, 21, GOLD);
            let lines = crate::title_screen::wrap(&e.description, font, details.w-12.0);
            let visible_lines = ((details.h-56.0)/20.0).floor().max(1.0) as usize;
            if details.contains(pointer) && wheel != 0.0 {
                self.description_offset = self.description_offset.saturating_add_signed(if wheel > 0.0 {-3} else {3});
            }
            self.description_offset = self.description_offset.min(lines.len().saturating_sub(visible_lines));
            let mut baseline = details.y+53.0;
            for line in lines.iter().skip(self.description_offset).take(visible_lines) {
                text(font, line, details.x+6.0, baseline, 16, LABEL);
                baseline += 20.0;
            }
            if let Some(art) = e.image.and_then(|id| self.art.get(id)) {
                let space = Rect::new(details.x+4.0, baseline, details.w-8.0, (details.bottom()-baseline-4.0).max(0.0));
                if space.h > 0.0 { contain(&art, space, false); }
            }
        }
        if !self.campaigns_only && !self.campaigns.is_empty() {
            self.art.button(toggle, if self.show_levels {"Campaigns"} else {"Scenarios"}, pointer);
        }
        if let Some(message) = error {
            let left = if self.campaigns_only || self.campaigns.is_empty() { panel.x } else { toggle.right()+8.0 };
            let message = fit(self.art.font.as_ref(), message.into(), play.x-left-8.0, 15);
            self.art.label(&message, left, bottom+24.0, 15, Color::from_rgba(255,180,146,255));
        }
        self.art.button(play, "game^Play", pointer);
        self.art.button(cancel, "Cancel", pointer);
        if self.campaigns_only {
            self.difficulty_menu.select(entry.and_then(|e| e.descriptor), &mut self.art.locale, &self.art.roots[0]);
            let box_width = (window.w * 0.233).round();
            let control = Rect::new(window.right()-18.0-box_width, window.bottom()-98.0, box_width, 30.0);
            self.difficulty_menu.draw(&mut self.art, control, pointer, !self.editing);
        }
        action
    }
}

fn fill(rect: Rect, color: Color) { draw_rectangle(rect.x, rect.y, rect.w, rect.h, color); }
fn outline(rect: Rect, color: Color) { draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 2.0, color); }
fn image(tex: &Texture2D, rect: Rect) {
    draw_texture_ex(tex, rect.x, rect.y, WHITE, DrawTextureParams {
        dest_size: Some(vec2(rect.w,rect.h)), ..Default::default()
    });
}
fn contain(tex: &Texture2D, rect: Rect, upscale: bool) {
    let mut scale = (rect.w / tex.width()).min(rect.h / tex.height());
    if !upscale { scale = scale.min(1.0); }
    let size = tex.size()*scale;
    let xy = (rect.center()-size*0.5).round();
    image(tex, Rect::new(xy.x,xy.y,size.x,size.y));
}

struct Art {
    locale: Locale,
    roots: Vec<PathBuf>,
    font: Option<Font>,
    textures: HashMap<String, Option<Texture2D>>,
}
impl Art {
    fn new(root: &Path, campaign: &Node, locale: Locale) -> Self {
        let mut roots = vec![root.to_owned(), root.join("images")];
        for (_, node) in campaign.children.iter().filter(|(tag,_)| tag == "binary_path") {
            if let Some(path) = node.attribute("path").map(Path::new).filter(|p| relative(p)) {
                roots.push(root.join(path).join("images"));
            }
        }
        roots.push(root.join("data/core/images"));
        let font = fs::read(root.join("fonts").join(locale::font_name(&locale.effective))).ok().and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok());
        Self { locale, roots, font, textures: HashMap::new() }
    }
    fn get(&mut self, id: &str) -> Option<Texture2D> {
        if let Some(image) = self.textures.get(id) { return image.clone(); }
        let path = Path::new(id.split('~').next()?);
        let texture = relative(path).then(|| self.roots.iter().find_map(|root| fs::read(root.join(path)).ok())).flatten()
            .and_then(|bytes| Image::from_file_with_format(&bytes,None).ok())
            .map(|image| Texture2D::from_image(&image));
        self.textures.insert(id.into(), texture.clone());
        texture
    }
    fn label(&self, label: &str, x: f32, y: f32, size: u16, color: Color) {
        text(self.font.as_ref(), label, x, y, size, color);
    }
    fn window(&mut self, window: Rect) {
        fill(window, Color::from_rgba(26, 31, 38, 255));
        if let Some(tile) = self.get("dialogs/opaque-background.png") {
            let mut y = window.y+2.0;
            while y < window.bottom()-2.0 {
                let h = tile.height().min(window.bottom()-2.0-y);
                let mut x = window.x+2.0;
                while x < window.right()-2.0 {
                    let w = tile.width().min(window.right()-2.0-x);
                    draw_texture_ex(&tile,x,y,WHITE,DrawTextureParams {
                        source: Some(Rect::new(0.0,0.0,w,h)), ..Default::default()
                    });
                    x += tile.width();
                }
                y += tile.height();
            }
        }
        outline(window, EDGE);
    }
    fn button(&mut self, rect: Rect, label: &str, pointer: Vec2) {
        let asset = if rect.contains(pointer) {
            if is_mouse_button_down(MouseButton::Left) { "buttons/large-button-active-pressed.png" }
            else { "buttons/large-button-active.png" }
        } else { "buttons/large-button.png" };
        fill(rect, Color::from_rgba(38,47,60,255));
        if let Some(image_asset) = self.get(asset) { image(&image_asset, rect); }
        let label = self.locale.ui(label);
        let label = label.split_once('^').map_or(label, |(_, display)| display);
        let size = measure_text(label, self.font.as_ref(), 17, 1.0);
        self.label(label, rect.center().x-size.width*0.5, rect.center().y+6.0, 17, GOLD);
    }
}

fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|p| matches!(p, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{campaign, wml};
    #[test]
    fn campaign_selection_resolves_the_authored_opening_and_preserves_scenario_access() {
        let document = wml::parse("[campaign]\nname=The South Guard\nfirst_scenario=opening\nicon=data/icon.png\nimage=data/art.png\ndescription= _ \"<i>Deoran</i> rides south.\"\n[/campaign]\n[campaign]\nfirst_scenario=absent\n[/campaign]\n[scenario]\nid=side_route\nname=Crossroads\n[/scenario]\n[scenario]\nid=opening\nname=First Ride\n[/scenario]").unwrap();
        let scenarios: Vec<_> = campaign::scenarios(&document).collect();
        let (campaigns, levels) = entries(&document,&scenarios, &mut Locale::default(), Path::new(""));
        assert_eq!(campaigns.len(), 1);
        assert_eq!((campaigns[0].name.as_str(), campaigns[0].scenario), ("The South Guard", 1));
        assert_eq!((campaigns[0].icon, campaigns[0].image), (Some("data/icon.png"), Some("data/art.png")));
        assert_eq!(campaigns[0].description, "Deoran rides south.");
        assert_eq!(levels[filtered(&levels,"CROSS")[0]].scenario, 0);
        assert_eq!(filtered(&levels,"missing"), Vec::<usize>::new());
    }
}

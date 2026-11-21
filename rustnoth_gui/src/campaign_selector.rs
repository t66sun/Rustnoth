//! Campaign selection using the artwork and two-pane layout of campaign_dialog.cfg.
#[path = "campaign_difficulty.rs"]
mod difficulty;
#[path = "campaign_combat_menu.rs"]
pub(super) mod combat;
#[path = "campaign_mod_menu.rs"]
mod mods;
#[path = "campaign_sort.rs"]
mod sort;
#[path = "campaign_layout.rs"]
mod layout;
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
    date: sort::Date,
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
            date: sort::Date::read(node.attribute("start_year")),
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
                date: sort::Date::Undated,
                description: plain(&locale.attribute(install, node, "description", id)),
                icon: None, image: None, background: None, scenario, descriptor: None,
            }
        }).collect();
    (campaigns, levels)
}

fn filtered(entries: &[Entry<'_>], order: impl Iterator<Item = usize>, filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    order.filter(|&i| entries[i].name.to_lowercase().contains(&needle)).collect()
}

pub(super) struct Selector<'a> {
    campaigns: Vec<Entry<'a>>,
    ordering: sort::Order,
    campaign_rows: Vec<usize>,
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
    combat_menu: combat::Menu,
    mod_menu: mods::Menu<'a>,
}

impl<'a> Selector<'a> {
    pub fn new(root: &Path, campaign: &'a Node, scenarios: &[(&'a str, &'a str)]) -> std::io::Result<Self> {
        let selected = crate::display_preferences::config_directory().and_then(|dir| locale::saved(&dir)).unwrap_or_default();
        let mut language = Locale::load(root, selected)?;
        let (campaigns, levels) = entries(campaign, scenarios, &mut language, root);
        let mod_menu = mods::Menu::new(campaign, &mut language, root);
        Ok(Self {
            ordering: sort::Order::Rank, campaign_rows: (0..campaigns.len()).collect(),
            campaigns_only: false, show_levels: campaigns.is_empty(), campaigns, levels, cursor: 0, first: 0,
            query: String::new(), editing: false, description_offset: 0, last_click: None,
            art: Art::new(root, campaign, language),
            difficulty_menu: difficulty::Menu::default(),
            combat_menu: combat::Menu::default(),
            mod_menu,
        })
    }

    pub(super) fn combat_mode(&self) -> combat::Mode { self.combat_menu.mode() }
    pub(super) fn modifications(&self) -> Vec<Node> { self.mod_menu.selected() }
    pub(super) fn restore_modifications(&mut self) -> std::io::Result<()> { self.mod_menu.restore() }
    pub(super) fn remember_modifications(&mut self) -> std::io::Result<()> { self.mod_menu.remember() }

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
        clear_background(BLACK);
        let entries = if self.show_levels { &self.levels } else { &self.campaigns };
        let name_width = entries.iter().map(|entry|
            measure_text(&entry.name, self.art.font.as_ref(), 17, 1.0).width.ceil() as i32
        ).max().unwrap_or(0);
        let layout::Layout { window, panel, list, details, search, play, cancel } =
            layout::Layout::new(w, h, name_width as f32);
        let bottom = play.y;
        let toggle = Rect::new(panel.x, bottom, 120.0, 38.0);
        let (mx, my) = mouse_position();
        let pointer = vec2(mx/scale, my/scale);
        let modal = self.difficulty_menu.is_open() || self.combat_menu.is_open() || self.mod_menu.is_open();
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
        let mut rows = if self.show_levels {
            filtered(entries, 0..entries.len(), &self.query)
        } else {
            filtered(entries, self.campaign_rows.iter().copied(), &self.query)
        };
        let headers = sort::headers(list);
        if !self.show_levels && click
            && let Some((field, _)) = headers.iter().find(|(_, rect)| rect.contains(pointer))
        {
            let selected = rows.get(self.cursor).copied();
            self.ordering = self.ordering.toggle(*field);
            self.campaign_rows = self.ordering.rows(entries, &self.art.locale.effective);
            rows = filtered(entries, self.campaign_rows.iter().copied(), &self.query);
            self.cursor = selected.and_then(|idx| rows.iter().position(|&r| r == idx)).unwrap_or(0);
            self.last_click = None;
        }
        let row_h = if self.show_levels { 44.0 } else { 82.0 };
        let top = list.y+30.0;
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
        let body = Rect::new(list.x, top, list.w - 15.0, list.bottom() - top);
        let bar = Rect::new(body.right(), body.y, 15.0, body.h);
        let row_click = click && body.contains(pointer);
        if !modal && is_mouse_button_down(MouseButton::Left) && bar.contains(pointer) && rows.len() > visible {
            self.first = (((pointer.y - bar.y) / bar.h) * (rows.len() - visible) as f32).round() as usize;
            self.cursor = self.cursor.clamp(self.first, (self.first + visible - 1).min(last));
        }
        let mut confirmed = key(KeyCode::Enter) || (!self.editing && key(KeyCode::Space))
            || click && play.contains(pointer);
        if row_click {
            let row = ((pointer.y-top)/row_h) as usize;
            let index = self.first+row;
            if index < rows.len() {
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
        self.art.label(self.art.locale.ui("Play a Campaign"), panel.x, window.y+37.0, 22, GOLD);
        fill(search, Color::from_rgba(10, 14, 21, 255));
        outline(search, if self.editing { GOLD } else { EDGE });
        let query_label = fit(self.art.font.as_ref(), self.query.clone(), search.w-16.0, 16);
        self.art.label(&query_label, search.x+8.0, search.y+21.0, 16, LABEL);
        let backdrop = entry.and_then(|e| e.background).unwrap_or("story/landscape-battlefield_nohumans.webp");
        if let Some(tex) = self.art.get(backdrop) {
            let dst = layout::backdrop(panel, tex.size());
            clip(Some(panel), scale);
            draw_texture_ex(&tex, dst.x, dst.y, Color::from_rgba(255,255,255,63),
                DrawTextureParams { dest_size: Some(dst.size()), ..Default::default() });
            clip(None, scale);
        }
        fill(panel, Color::from_rgba(0, 0, 0, 138));
        frame(panel);
        fill(Rect::new(list.x, list.y, list.w, 30.0), Color::from_rgba(0, 0, 0, 89));
        if self.show_levels {
            self.art.label(self.art.locale.ui("Name"), list.x+10.0, list.y+21.0, 16, GOLD);
        } else {
            for (field, rect) in headers {
                sort::header(&mut self.art, field, rect, self.ordering, pointer);
            }
        }
        clip(Some(body), scale);
        for (row, &index) in rows.iter().skip(self.first).take(visible+1).enumerate() {
            let rect = Rect::new(body.x, top+row as f32*row_h, body.w, row_h);
            let item = &entries[index];
            fill(rect, Color::from_rgba(0,0,0,89));
            if self.cursor == self.first+row || rect.contains(pointer) {
                if let Some(texture) = self.art.get("dialogs/selection-background-fancy.png") {
                    let alpha = if self.cursor == self.first+row {255} else {191};
                    draw_texture_ex(&texture, rect.x, rect.y, Color::from_rgba(255,255,255,alpha),
                        DrawTextureParams { dest_size: Some(rect.size()), ..Default::default() });
                }
                if self.cursor == self.first+row {
                    fill(Rect::new(rect.x,rect.y,2.,rect.h), EDGE);
                    fill(Rect::new(rect.x,rect.y+1.,2.,rect.h-2.), GOLD);
                }
            }
            let mut label_x = rect.x+12.0;
            if !self.show_levels {
                let cell = Rect::new(rect.x+8.0, rect.y+5.0, 72.0, 72.0);
                if let Some(icon) = item.icon.and_then(|id| self.art.get(id)) { contain(&icon, cell, false); }
                label_x = cell.right()+5.0;
            }
            let name = fit(self.art.font.as_ref(), item.name.clone(), rect.right()-label_x-4.0, 17);
            self.art.label(&name, label_x, rect.center().y+6.0, 17, LABEL);
        }
        clip(None, scale);
        self.art.scrollbar(bar, self.first, row_h, rows.len(), scale);
        if rows.is_empty() { self.art.label(self.art.locale.ui("No matches"), list.x+12.0, top+28.0, 16, LABEL); }
        if let Some(e) = entry {
            let portrait = e.image.and_then(|id| self.art.get(id));
            let font = self.art.font.as_ref();
            let lines = crate::title_screen::wrap(&e.description, font, details.w-10.0);
            let text_h = lines.len() as f32 * 21.0;
            let image_size = portrait.as_ref().map(Texture2D::size).unwrap_or(Vec2::ZERO);
            let width_scale = ((details.w-10.0) / image_size.x.max(1.0)).min(1.0);
            let image_size = image_size * width_scale;
            let content_h = 20.0 + text_h + image_size.y;
            let overflow = (content_h - details.h).max(0.);
            if details.contains(pointer) && wheel != 0.0 {
                self.description_offset = self.description_offset.saturating_add_signed(if wheel > 0.0 {-3} else {3});
            }
            self.description_offset = self.description_offset.min((overflow / 21.0).ceil() as usize);
            let offset = (self.description_offset as f32 * 21.0).min(overflow);
            clip(Some(details), scale);
            let mut baseline = details.y + 5.0 + 17.0 - offset;
            for line in &lines {
                text(font, line, details.x+5.0, baseline, 17, LABEL);
                baseline += 21.0;
            }
            if let Some(portrait) = portrait {
                image(&portrait, layout::portrait(details, text_h, image_size, offset));
            }
            clip(None, scale);
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
            let box_width = (window.w * 0.233).floor();
            let control = Rect::new(window.right()-19.0-box_width, window.bottom()-98.0, box_width, 30.0);
            let combat = Rect::new(window.x + (window.w * 0.428).floor(), control.y, box_width, control.h);
            let combat_open = self.combat_menu.is_open();
            let mods_open = self.mod_menu.is_open();
            let difficulty_open = self.difficulty_menu.is_open();
            self.difficulty_menu.draw(&mut self.art, control, if combat_open || mods_open { vec2(-1.,-1.) } else { pointer },
                !self.editing && !combat_open && !mods_open, vec2(w, h));
            self.combat_menu.draw(&mut self.art, combat, pointer, vec2(w, h), !difficulty_open && !self.difficulty_menu.is_open() && !mods_open);
            let available = !combat_open && !self.combat_menu.is_open() && !difficulty_open && !self.difficulty_menu.is_open();
            self.mod_menu.draw(&mut self.art, panel.x, control.y, box_width, pointer, vec2(w, h), available);
        }
        action
    }
}

fn fill(rect: Rect, color: Color) { draw_rectangle(rect.x, rect.y, rect.w, rect.h, color); }
fn clip(rect: Option<Rect>, scale: f32) {
    let p = scale * screen_dpi_scale();
    let bounds = rect.map(|r| ((r.x*p).round() as i32, (r.y*p).round() as i32,
        (r.w*p).round() as i32, (r.h*p).round() as i32));
    unsafe { get_internal_gl().quad_gl.scissor(bounds); }
}

fn frame(r: Rect) {
    for (inset, color) in [(0., Color::from_rgba(16,22,35,255)), (1., EDGE), (2., Color::from_rgba(16,22,35,255))] {
        let x = r.x + inset;
        let y = r.y + inset;
        let w = r.w - inset * 2.;
        let h = r.h - inset * 2.;
        fill(Rect::new(x, y, w, 1.), color);
        fill(Rect::new(x, y+h-1., w, 1.), color);
        fill(Rect::new(x, y, 1., h), color);
        fill(Rect::new(x+w-1., y, 1., h), color);
    }
}
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
            .and_then(|bytes| crate::artwork::image(&bytes).ok())
            .map(|mut image| {
                if id.ends_with("~GS()") {
                    for px in image.get_image_data_mut() {
                        if px[3] != 0 {
                            let gray = ((77*u16::from(px[0]) + 150*u16::from(px[1]) + 29*u16::from(px[2])) / 256) as u8;
                            px[..3].fill(gray);
                        }
                    }
                }
                let texture = Texture2D::from_image(&image);
                texture.set_filter(FilterMode::Nearest);
                texture
            });
        self.textures.insert(id.into(), texture.clone());
        texture
    }
    fn label(&self, label: &str, x: f32, y: f32, size: u16, color: Color) {
        text(self.font.as_ref(), label, x, y, size, color);
    }
    fn window(&mut self, window: Rect) {
        fill(window, Color::from_rgba(16, 22, 35, 255));
        frame(window);
        self.tile("dialogs/opaque-background.png",
            Rect::new(window.x+2., window.y+2., window.w-4., window.h-4.));
    }
    fn tile(&mut self, asset: &str, bounds: Rect) {
        if let Some(tile) = self.get(asset) {
            let mut y = bounds.y;
            while y < bounds.bottom() {
                let h = tile.height().min(bounds.bottom()-y);
                let mut x = bounds.x;
                while x < bounds.right() {
                    let w = tile.width().min(bounds.right()-x);
                    draw_texture_ex(&tile,x,y,WHITE,DrawTextureParams {
                        source: Some(Rect::new(0.0,0.0,w,h)), ..Default::default()
                    });
                    x += tile.width();
                }
                y += tile.height();
            }
        }
    }
    fn scrollbar(&mut self, bar: Rect, first: usize, item_height: f32, count: usize, scale: f32) {
        fill(bar, Color::from_rgba(0, 0, 0, 89));
        let content_h = (count as f32 * item_height).max(bar.h);
        let length = (bar.h * bar.h / content_h).floor().max(11.).min(bar.h);
        let pos = (bar.h * first as f32 * item_height / content_h).floor().min(bar.h-length);
        let y = bar.y + pos;
        clip(Some(bar), scale);
        for (name, top, height) in [("scrolltop", y, 2.), ("scrollmid", y+2., length-2.), ("scrollbottom", y+length, 2.)] {
            if let Some(tex) = self.get(&format!("buttons/scrollbars/{name}.png")) {
                image(&tex, Rect::new(bar.x,top,bar.w,height));
            }
        }
        clip(None, 1.);
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
        assert_eq!(levels[filtered(&levels,0..levels.len(),"CROSS")[0]].scenario, 0);
        assert_eq!(filtered(&levels,0..levels.len(),"missing"), Vec::<usize>::new());
    }
}

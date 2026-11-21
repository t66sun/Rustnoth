//! Campaign selection using the artwork and two-pane layout of campaign_dialog.cfg.
#[path = "campaign_difficulty.rs"]
mod difficulty;
#[path = "campaign_combat_menu.rs"]
pub(super) mod combat;
#[path = "campaign_sort.rs"]
mod sort;
#[path = "campaign_layout.rs"]
mod layout;
#[path = "campaign_backdrop.rs"]
mod backdrop;
#[path = "campaign_scroll.rs"]
mod scrolling;
#[path = "campaign_text.rs"]
mod campaign_text;
use campaign_text::{Caption, Typography};
use std::{collections::HashMap, fs, path::{Component, Path, PathBuf}};

use engine_lite::wml::Node;
use macroquad::prelude::*;
use crate::dialog::{fit, setup_camera, text, ListAction};
use crate::title_screen::locale::{self, Locale};
use crate::title_screen::button_canvas::{self, State};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Footer { Play, Cancel }

fn footer_input(pressed: &mut Option<Footer>, hover: Option<Footer>, down: bool, up: bool) -> Option<Footer> {
    if down { *pressed = hover; }
    if up && pressed.take() == hover { hover } else { None }
}

const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
const LABEL: Color = Color::from_rgba(221, 221, 221, 255);
const EDGE: Color = Color::from_rgba(114, 79, 46, 255);

struct Entry<'a> {
    name: String,
    search_text: String,
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
        let name = locale.attribute(install, node, "name", id);
        let description = locale.attribute(install, node, "description", "");
        let abbreviation = locale.attribute(install, node, "abbrev", "");
        let search_text = campaign_search_text(node, &name, &description, &abbreviation);
        Some(Entry {
            name, search_text,
            date: sort::Date::read(node.attribute("start_year")),
            description,
            icon: node.attribute("icon"), image: node.attribute("image"),
            background: node.attribute("background"), scenario, descriptor: Some(node),
        })
    }).collect();
    let levels = root.children.iter().filter(|(tag, node)| tag == "scenario" && node.attribute("id").is_some())
        .zip(scenarios).enumerate().map(|(scenario, ((_, node), &(id, _)))| {
            // Scenario bundles need not include a [campaign] descriptor or a description.
            let name = locale.attribute(install, node, "name", id);
            Entry {
                search_text: name.to_lowercase(), name,
                date: sort::Date::Undated,
                description: locale.attribute(install, node, "description", id),
                icon: None, image: None, background: None, scenario, descriptor: None,
            }
        }).collect();
    (campaigns, levels)
}

fn campaign_search_text(node: &Node, name: &str, description: &str, abbreviation: &str) -> String {
    [name, node.attribute("name").unwrap_or(""), description,
        node.attribute("description").unwrap_or(""), abbreviation,
        node.attribute("abbrev").unwrap_or("")].join("\n").to_lowercase()
}

fn filtered(entries: &[Entry<'_>], order: impl Iterator<Item = usize>, filter: &str) -> Vec<usize> {
    let query = filter.to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    order.filter(|&i| words.iter().all(|word| entries[i].search_text.contains(*word))).collect()
}

fn selected_row(rows: &[usize], selected: Option<usize>) -> usize {
    selected.and_then(|id| rows.iter().position(|&row| row == id)).unwrap_or(0)
}

pub(super) struct Selector<'a> {
    campaigns: Vec<Entry<'a>>,
    ordering: sort::Order,
    campaign_rows: Vec<usize>,
    levels: Vec<Entry<'a>>,
    pub(super) campaigns_only: bool,
    show_levels: bool,
    cursor: usize,
    scroll: scrolling::Scroll,
    query: String,
    editing: bool,
    description_offset: usize,
    last_click: Option<(usize, f64)>,
    footer_press: Option<Footer>,
    art: Art,
    difficulty_menu: difficulty::Menu<'a>,
    combat_menu: combat::Menu,
}

impl<'a> Selector<'a> {
    pub fn new(root: &Path, campaign: &'a Node, scenarios: &[(&'a str, &'a str)]) -> std::io::Result<Self> {
        let selected = crate::display_preferences::config_directory().and_then(|dir| locale::saved(&dir)).unwrap_or_default();
        let mut language = Locale::load(root, selected)?;
        let (campaigns, levels) = entries(campaign, scenarios, &mut language, root);
        // The opening title shortcut can still be in macroquad's text queue.
        while get_char_pressed().is_some() {}
        let editing = !campaigns.is_empty();
        Ok(Self {
            footer_press: None,
            ordering: sort::Order::Rank, campaign_rows: (0..campaigns.len()).collect(),
            campaigns_only: false, show_levels: campaigns.is_empty(), campaigns, levels, cursor: 0, scroll: Default::default(),
            query: String::new(), editing, description_offset: 0, last_click: None,
            art: Art::new(root, campaign, language),
            difficulty_menu: difficulty::Menu::default(),
            combat_menu: combat::Menu::default(),
        })
    }

    pub(super) fn combat_mode(&self) -> combat::Mode { self.combat_menu.mode() }

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
            self.art.typography.width(&entry.name, Caption::Row, self.art.font.as_ref()) as i32
        ).max().unwrap_or(0);
        let layout::Layout { window, panel, list, details, search, play, cancel } =
            layout::Layout::new(w, h, name_width as f32);
        let bottom = play.y;
        let toggle = Rect::new(panel.x, bottom, 120.0, 38.0);
        let (mx, my) = mouse_position();
        let pointer = vec2(mx/scale, my/scale);
        let modal = self.difficulty_menu.is_open() || self.combat_menu.is_open();
        let key = |code| !modal && is_key_pressed(code);
        let click = !modal && is_mouse_button_pressed(MouseButton::Left);
        if key(KeyCode::Escape) { return ListAction::Back; }
        if !self.campaigns_only && !self.campaigns.is_empty() && (click && toggle.contains(pointer) || key(KeyCode::Tab)) {
            self.show_levels = !self.show_levels;
            self.editing = false;
            self.query.clear();
            self.cursor = 0;
            self.scroll = Default::default();
            self.description_offset = 0;
            self.last_click = None;
        }
        if click { self.editing = search.contains(pointer); }
        let focus_shortcut = key(KeyCode::F) && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl));
        if focus_shortcut {
            self.editing = true;
        }
        let entries = if self.show_levels { &self.levels } else { &self.campaigns };
        let matching = |query: &str| if self.show_levels {
            let needle = query.trim().to_lowercase();
            (0..entries.len()).filter(|&i| entries[i].search_text.contains(&needle)).collect()
        } else {
            filtered(entries, self.campaign_rows.iter().copied(), query)
        };
        let mut rows = matching(&self.query);
        let selected = rows.get(self.cursor).copied();
        let mut changed = false;
        while let Some(c) = get_char_pressed() {
            if self.editing && !modal && !focus_shortcut && !c.is_control() {
                self.query.push(c);
                changed = true;
            }
        }
        if self.editing && key(KeyCode::Backspace) {
            changed |= self.query.pop().is_some();
        }
        if changed {
            rows = matching(&self.query);
            self.cursor = selected_row(&rows, selected);
            if rows.get(self.cursor).copied() != selected { self.description_offset = 0; }
            self.last_click = None;
        }
        let widths = ["Name", "Timeline"].map(|name|
            self.art.typography.width(self.art.locale.ui(name), Caption::Header, self.art.font.as_ref()).ceil());
        let headers = sort::headers(list, widths);
        let mut reveal = changed;
        if !self.show_levels && click
            && let Some((field, _)) = headers.iter().find(|(_, rect)| rect.contains(pointer))
        {
            let selected = rows.get(self.cursor).copied();
            self.ordering = self.ordering.toggle(*field);
            self.campaign_rows = self.ordering.rows(entries, &self.art.locale.effective);
            rows = filtered(entries, self.campaign_rows.iter().copied(), &self.query);
            self.cursor = selected_row(&rows, selected);
            reveal = true;
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
        let body = Rect::new(list.x, top, list.w - 15.0, list.bottom() - top);
        let bar = Rect::new(body.right(), body.y, 15.0, body.h);
        if reveal || step != 0 || key(KeyCode::Home) || key(KeyCode::End) {
            self.scroll.reveal(self.cursor, row_h, body.h);
        }
        let content = row_h * rows.len() as f32;
        self.scroll.update(bar, content, pointer, click, !modal && is_mouse_button_down(MouseButton::Left),
            if list.contains(pointer) { wheel } else { 0. });
        let row_click = click && body.contains(pointer);
        let hover = if modal { None }
            else if cancel.contains(pointer) { Some(Footer::Cancel) }
            else if play.contains(pointer) && !rows.is_empty() { Some(Footer::Play) }
            else { None };
        let released = footer_input(&mut self.footer_press, hover, click, is_mouse_button_released(MouseButton::Left));
        if released == Some(Footer::Cancel) { return ListAction::Back; }
        let mut confirmed = key(KeyCode::Enter) || (!self.editing && key(KeyCode::Space))
            || released == Some(Footer::Play);
        if row_click {
            let index = ((pointer.y-top+self.scroll.offset)/row_h) as usize;
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
        self.art.typography.draw(self.art.locale.ui("Play a Campaign"), Caption::Title,
            Rect::new(panel.x, window.y+15., panel.w, 30.), self.art.font.as_ref());
        fill(search, Color::from_rgba(10, 14, 21, 255));
        outline(search, if self.editing { GOLD } else { EDGE });
        let query_label = fit(self.art.font.as_ref(), self.query.clone(), search.w-16.0, 16);
        self.art.label(&query_label, search.x+8.0, search.y+21.0, 16, LABEL);
        let backdrop = entry.and_then(|e| e.background).unwrap_or("story/landscape-battlefield_nohumans.webp");
        let shaded = self.art.backdrop(backdrop, window, panel);
        frame(panel);
        backdrop::draw_list(&shaded, panel, Rect::new(list.x, list.y, list.w, 30.0));
        if self.show_levels {
            self.art.label(self.art.locale.ui("Name"), list.x+10.0, list.y+21.0, 16, GOLD);
        } else {
            for (field, rect) in headers {
                sort::header(&mut self.art, field, rect, self.ordering, pointer);
            }
        }
        clip(Some(body), scale);
        let first = (self.scroll.offset / row_h) as usize;
        for (row, &index) in rows.iter().enumerate().skip(first).take(visible+2) {
            let rect = Rect::new(body.x, top+row as f32*row_h-self.scroll.offset, body.w, row_h);
            let item = &entries[index];
            backdrop::draw_list(&shaded, panel, rect);
            if self.cursor == row || body.contains(pointer) && rect.contains(pointer) {
                if let Some(texture) = self.art.get("dialogs/selection-background-fancy.png") {
                    let alpha = if self.cursor == row {255} else {191};
                    draw_texture_ex(&texture, rect.x, rect.y, Color::from_rgba(255,255,255,alpha),
                        DrawTextureParams { dest_size: Some(rect.size()), ..Default::default() });
                }
                if self.cursor == row {
                    fill(Rect::new(rect.x,rect.y,2.,rect.h), EDGE);
                    fill(Rect::new(rect.x,rect.y+1.,2.,rect.h-2.), Color::from_rgba(185,158,75,255));
                }
            }
            let mut label_x = rect.x+12.0;
            if !self.show_levels {
                let cell = Rect::new(rect.x+8.0, rect.y+5.0, 72.0, 72.0);
                if let Some(icon) = item.icon.and_then(|id| self.art.get(id)) { contain(&icon, cell, false); }
                label_x = cell.right()+5.0;
            }
            let name = fit(self.art.font.as_ref(), item.name.clone(), rect.right()-label_x-4.0, 17);
            self.art.typography.draw(&name, Caption::Row,
                Rect::new(label_x, rect.y, rect.right()-label_x-4., rect.h), self.art.font.as_ref());
        }
        clip(None, scale);
        backdrop::draw_list(&shaded, panel, bar);
        let thumb = self.scroll.thumb(bar, content);
        let suffix = if self.scroll.grab.is_some() { "-pressed" } else if thumb.contains(pointer) { "-active" } else { "" };
        self.art.scrollbar(bar, thumb, suffix, scale);
        if rows.is_empty() { self.art.label(self.art.locale.ui("No matches"), list.x+12.0, top+28.0, 16, LABEL); }
        if let Some(e) = entry {
            let portrait = e.image.and_then(|id| self.art.get(id));
            let font = self.art.font.as_ref();
            let align = e.descriptor.and_then(|cfg| cfg.attribute("description_alignment"));
            let paragraph = self.art.typography.description(&e.description, details.w-10., align, font);
            let text_h = paragraph.height();
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
            paragraph.draw(details.x + 5., details.y + 5. - offset, font);
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
        for (which, rect, label) in [(Footer::Play, play, "game^Play"), (Footer::Cancel, cancel, "Cancel")] {
            let state = if which == Footer::Play && entry.is_none() { State::Disabled }
                else if hover != Some(which) { State::Enabled }
                else if self.footer_press == hover { State::Pressed }
                else { State::Focused };
            self.art.footer_button(rect, label, state);
        }
        if self.campaigns_only {
            self.difficulty_menu.select(entry.and_then(|e| e.descriptor), &mut self.art.locale, &self.art.roots[0]);
            let box_width = (window.w * 0.233).floor();
            let control = Rect::new(window.right()-19.0-box_width, window.bottom()-98.0, box_width, 30.0);
            let combat = Rect::new(window.x + (window.w * 0.428).floor(), control.y, box_width, control.h);
            let combat_open = self.combat_menu.is_open();
            self.difficulty_menu.draw(&mut self.art, control, if combat_open { vec2(-1.,-1.) } else { pointer },
                !self.editing && !combat_open, vec2(w, h));
            self.combat_menu.draw(&mut self.art, combat, pointer, vec2(w, h), !self.difficulty_menu.is_open());
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
    typography: Typography,
    locale: Locale,
    roots: Vec<PathBuf>,
    font: Option<Font>,
    textures: HashMap<String, Option<Texture2D>>,
    team_colors: Vec<u8>,
    backdrop: Option<backdrop::Backdrop>,
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
        let typography = Typography::new(root, locale.game("Lato"));
        let team_colors = fs::read(root.join("data/core/team-colors.cfg")).unwrap_or_default();
        Self { locale, roots, font, typography, textures: HashMap::new(), team_colors, backdrop: None }
    }
    fn pixels(&self, id: &str) -> Option<Image> {
        if id.contains('~') && let Some(icon) = crate::title_screen::icon_pixels(id, &self.team_colors, &|base| {
            if !relative(base) { return None; }
            self.roots.iter().map(|dir| dir.join(base)).find(|path| path.is_file())
        }) {
            return Some(Image { width: icon.width().try_into().ok()?, height: icon.height().try_into().ok()?,
                bytes: icon.into_raw() });
        }
        let path = Path::new(id.split('~').next()?);
        relative(path).then(|| self.roots.iter().find_map(|root| fs::read(root.join(path)).ok())).flatten()
            .and_then(|bytes| crate::artwork::image(&bytes).ok())
    }
    fn get(&mut self, id: &str) -> Option<Texture2D> {
        if let Some(image) = self.textures.get(id) { return image.clone(); }
        let texture = self.pixels(id)
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
    fn backdrop(&mut self, id: &str, window: Rect, panel: Rect) -> Texture2D {
        if let Some(cached) = &self.backdrop && cached.id == id && cached.bounds == panel {
            return cached.draw();
        }
        let tile = self.pixels("dialogs/opaque-background.png");
        let scene = self.pixels(id);
        self.backdrop.insert(backdrop::Backdrop::new(id, window, panel, tile.as_ref(), scene.as_ref())).draw()
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
    fn scrollbar(&mut self, bar: Rect, thumb: Rect, suffix: &str, scale: f32) {
        let y = thumb.y;
        let length = thumb.h;
        clip(Some(bar), scale);
        for (name, top, height) in [("scrolltop", y, 2.), ("scrollmid", y+2., length-2.), ("scrollbottom", y+length, 2.)] {
            if let Some(tex) = self.get(&format!("buttons/scrollbars/{name}{suffix}.png")) {
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
        let style = if label == "game^Play" { Caption::Play } else { Caption::Button };
        self.button_caption(rect, label, style);
    }
    fn footer_button(&mut self, rect: Rect, label: &str, state: State) {
        let asset = match state {
            State::Enabled => "buttons/button_normal/background.png",
            State::Focused => "buttons/button_normal/background-active.png",
            State::Pressed => "buttons/button_normal/background-pressed.png",
            State::Disabled => "buttons/button_normal/background.png~GS()",
        };
        if let Some(texture) = self.get(asset) { button_canvas::draw(rect, &texture, state); }
        let caption = if state == State::Disabled { Caption::DisabledPlay }
            else if label == "game^Play" { Caption::Play } else { Caption::Button };
        self.button_caption(rect, label, caption);
    }
    fn button_caption(&self, rect: Rect, label: &str, style: Caption) {
        let label = self.locale.ui(label);
        let label = label.split_once('^').map_or(label, |(_, display)| display);
        self.typography.draw(label, style, rect, self.font.as_ref());
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
    fn a_footer_click_requires_press_and_release_on_the_same_enabled_button() {
        let mut press = None;
        assert_eq!(footer_input(&mut press, Some(Footer::Play), true, false), None);
        assert_eq!(press, Some(Footer::Play));
        assert_eq!(footer_input(&mut press, Some(Footer::Cancel), false, true), None);
        assert_eq!(press, None);
        assert_eq!(footer_input(&mut press, Some(Footer::Cancel), true, false), None);
        assert_eq!(footer_input(&mut press, Some(Footer::Cancel), false, true), Some(Footer::Cancel));
        assert_eq!(footer_input(&mut press, None, true, false), None);
        assert_eq!(footer_input(&mut press, Some(Footer::Play), false, true), None);
    }
    #[test]
    fn campaign_filter_matches_every_word_across_translations_and_keeps_the_selected_entry() {
        let node = wml::parse("name=The South Guard\ndescription=Deoran rides south\nabbrev=TSG").unwrap();
        let other = wml::parse("name=Liberty\ndescription=Outlaws ride south\nabbrev=Lib").unwrap();
        let make_entry = |node: &Node, name: &str, description: &str, abbrev: &str| Entry {
            search_text: campaign_search_text(node, name, description, abbrev),
            name: name.into(), description: description.into(), date: sort::Date::Undated,
            icon: None, image: None, background: None, scenario: 0, descriptor: None,
        };
        let entries = [make_entry(&other, "自由", "亡命者", "自由篇"),
            make_entry(&node, "南疆哨卫", "迪奥兰出发", "南卫")];
        assert_eq!(filtered(&entries, 0..2, "  TSG 南疆 DeOrAn    出发  南卫 GUARD"), [1]);
        assert!(filtered(&entries, 0..2, "Liberty Deoran").is_empty());
        let rows = filtered(&entries, [1, 0].into_iter(), "SOUTH");
        assert_eq!(rows, [1, 0]);
        assert_eq!(selected_row(&rows, Some(0)), 1);
        assert_eq!(selected_row(&[1], Some(0)), 0);
        assert_eq!(selected_row(&[], None), 0);
    }
    #[test]
    fn campaign_selection_resolves_the_authored_opening_and_preserves_scenario_access() {
        let document = wml::parse("[campaign]\nname=The South Guard\nfirst_scenario=opening\nicon=data/icon.png\nimage=data/art.png\ndescription= _ \"<i>Deoran</i> rides south.\"\n[/campaign]\n[campaign]\nfirst_scenario=absent\n[/campaign]\n[scenario]\nid=side_route\nname=Crossroads\n[/scenario]\n[scenario]\nid=opening\nname=First Ride\n[/scenario]").unwrap();
        let scenarios: Vec<_> = campaign::scenarios(&document).collect();
        let (campaigns, levels) = entries(&document,&scenarios, &mut Locale::default(), Path::new(""));
        assert_eq!(campaigns.len(), 1);
        assert_eq!((campaigns[0].name.as_str(), campaigns[0].scenario), ("The South Guard", 1));
        assert_eq!((campaigns[0].icon, campaigns[0].image), (Some("data/icon.png"), Some("data/art.png")));
        assert_eq!(campaigns[0].description, "<i>Deoran</i> rides south.");
        assert_eq!(plain(&campaigns[0].description), "Deoran rides south.");
        assert_eq!(levels[filtered(&levels,0..levels.len(),"CROSS")[0]].scenario, 0);
        assert_eq!(filtered(&levels,0..levels.len(),"missing"), Vec::<usize>::new());
    }
}

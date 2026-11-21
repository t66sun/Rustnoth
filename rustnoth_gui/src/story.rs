//! Static campaign story parts shown before the board, including journey maps.
use std::{collections::HashMap, path::Path};

use engine_lite::wml::Node;
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;

use crate::{artwork::{self, scaled::Artwork}, dialog, render};

pub(super) struct Part {
    pub(super) title: String,
    pub(super) text: String,
    placement: Placement,
    layers: Vec<Layer>,
    markers: Vec<Marker>,
}

#[derive(Clone, Copy)]
enum Placement { Top, Middle, Bottom }

impl Placement {
    fn panel(self, screen: Vec2, title: bool) -> Rect {
        let start = if title { 100.0 } else { 0.0 };
        let height = (screen.y / 4.0).max(200.0).min(screen.y - start);
        let y = match self {
            Self::Top => start,
            Self::Middle => (screen.y + start - height) * 0.5,
            Self::Bottom => screen.y - height,
        };
        Rect::new(0.0, y, screen.x, height)
    }
}

struct Layer {
    file: String,
    base: bool,
    scale_x: bool,
    scale_y: bool,
    aspect: bool,
}

struct Marker {
    file: String,
    position: Vec2,
    centered: bool,
    resize: bool,
}

fn flag(node: &Node, key: &str, default: bool) -> bool {
    node.attribute(key).map_or(default, |s| matches!(s, "yes" | "true" | "1"))
}

fn literal<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.attribute(key).filter(|s| !s.is_empty() && !s.contains('$'))
}

fn plain_text(markup: &str) -> String {
    let mut text = String::new();
    let mut remaining = markup;
    while let Some((before, tag)) = remaining.split_once('<') {
        text.push_str(before);
        if let Some((_, after)) = tag.split_once('>') {
            remaining = after;
        } else {
            text.push('<');
            remaining = tag;
            break;
        }
    }
    text.push_str(remaining);
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&apos;", "'")
        .replace("&quot;", "\"").replace("&amp;", "&")
}

impl Layer {
    fn read(node: &Node, shortcut: bool) -> Option<Self> {
        let (image, scale, x, y) = if shortcut {
            ("background", "scale_background", "scale_background_horizontally", "scale_background_vertically")
        } else {
            ("image", "scale", "scale_horizontally", "scale_vertically")
        };
        Some(Self {
            file: literal(node, image)?.to_owned(),
            base: flag(node, "base_layer", false),
            scale_x: flag(node, scale, flag(node, x, true)),
            scale_y: flag(node, scale, flag(node, y, true)),
            aspect: flag(node, "keep_aspect_ratio", true),
        })
    }

    fn area(&self, original: Vec2, screen: Vec2) -> Rect {
        let (sw, sh) = (screen.x as i64, screen.y as i64);
        let (ow, oh) = (original.x as i64, original.y as i64);
        let width = if self.scale_y && self.aspect { (ow * sh / oh).min(sw) }
            else if self.scale_x { sw } else { ow };
        let height = if self.scale_x && self.aspect { (oh * sw / ow).min(sh) }
            else if self.scale_y { sh } else { oh };
        // The WFL canvas divides each side separately when centering a layer.
        Rect::new((sw / 2 - width / 2).max(0) as f32, (sh / 2 - height / 2).max(0) as f32,
                  width as f32, height as f32)
    }
}

pub(super) fn read(scenario: &Node, name: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    for (_, story) in scenario.children.iter().filter(|(tag, _)| tag == "story") {
        for (_, node) in story.children.iter().filter(|(tag, _)| tag == "part") {
            let mut layers: Vec<_> = Layer::read(node, true).into_iter().collect();
            let mut markers = Vec::new();
            for (tag, item) in &node.children {
                match tag.as_str() {
                    "background_layer" => layers.extend(Layer::read(item, false)),
                    "image" => {
                        if let (Some(file), Some(x), Some(y)) = (literal(item, "file"),
                            item.attribute("x").and_then(|s| s.parse().ok()),
                            item.attribute("y").and_then(|s| s.parse().ok())) {
                            markers.push(Marker { file: file.into(), position: vec2(x, y),
                                centered: flag(item, "centered", false), resize: flag(item, "resize_with_background", false) });
                        }
                    }
                    _ => {}
                }
            }
            let title = if flag(node, "show_title", node.attribute("title").is_some()) {
                plain_text(literal(node, "title").unwrap_or(name))
            } else { String::new() };
            let text = literal(node, "story").unwrap_or_default().to_owned();
            if !text.is_empty() || !layers.is_empty() || !markers.is_empty() || !title.is_empty() {
                let placement = match node.attribute("text_layout") { Some("top") => Placement::Top,
                    Some("middle") => Placement::Middle, _ => Placement::Bottom };
                parts.push(Part { title, text, placement, layers, markers });
            }
        }
    }
    parts
}

fn lines(font: Option<&Font>, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let next = if line.is_empty() { word.into() } else { format!("{line} {word}") };
            if !line.is_empty() && measure_text(&next, font, 19, 1.).width > width {
                lines.push(line);
                line = word.into();
            } else { line = next; }
        }
        lines.push(line);
    }
    lines
}

fn draw_art(part: &Part, layers: &HashMap<&str, Option<Artwork>>,
            markers: &HashMap<&str, Option<Texture2D>>, screen: Vec2) {
    let mut origin = Vec2::ZERO;
    let mut scale = Vec2::ONE;
    for (index, layer) in part.layers.iter().enumerate() {
        let Some(Some(image)) = layers.get(layer.file.as_str()) else { continue };
        let size = image.size();
        let rect = layer.area(size, screen);
        if index == 0 || layer.base {
            origin = rect.point();
            scale = rect.size() / size;
        }
        image.draw(rect, screen);
    }
    for marker in &part.markers {
        let Some(Some(image)) = markers.get(marker.file.as_str()) else { continue };
        let mut size = vec2(image.width(), image.height());
        if marker.resize { size *= scale; }
        let mut at = origin + marker.position * scale;
        if marker.centered { at -= size * 0.5; }
        draw_texture_ex(image, at.x, at.y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
    }
}

struct Skin {
    panel: [Option<Texture2D>; 3],
    controls: [[Option<Texture2D>; 3]; 3],
}

impl Skin {
    fn load(assets: &AssetCatalog) -> Self {
        let fetch = |name: &str| assets.resolve_image(Path::new(name)).and_then(|p| render::load_texture(&p, None));
        let panel = ["background","border-top","border-bottom"].map(|part| fetch(&format!("dialogs/translucent65-{part}.png")));
        let controls = ["icons/arrows/long_arrow_ornate_left","icons/arrows/long_arrow_ornate_right","buttons/button_normal/button_H22"]
            .map(|path| ["","-active","-pressed"].map(|state| fetch(&format!("{path}{state}.png"))));
        Self {panel, controls}
    }

    fn panel(&self, bounds: Rect, placement: Placement, title: bool) {
        let blit = |t: &Texture2D, r: Rect| draw_texture_ex(t,r.x,r.y,WHITE,DrawTextureParams { dest_size:Some(r.size()),..Default::default()});
        if let Some(bg)=&self.panel[0] { blit(bg,bounds); }
        else { draw_rectangle(bounds.x,bounds.y,bounds.w,bounds.h,Color::from_rgba(0,0,0,166)); }
        if (!matches!(placement,Placement::Top) || title) && let Some(top)=&self.panel[1] {
            blit(top,Rect::new(bounds.x,bounds.y,bounds.w,top.height()));
        }
        if !matches!(placement,Placement::Bottom) && let Some(bottom)=&self.panel[2] {
            blit(bottom,Rect::new(bounds.x,bounds.bottom()-bottom.height(),bounds.w,bottom.height()));
        }
    }

    fn control(&self, i: usize, r: Rect, pointer: Vec2, enabled: bool) -> bool {
        let hover = enabled && r.contains(pointer);
        let state = if hover && is_mouse_button_down(MouseButton::Left) {2} else {usize::from(hover)};
        if let Some(tex) = self.controls[i][state].as_ref().or(self.controls[i][0].as_ref()) {
            let size = if i == 2 { r.size() } else { vec2(tex.width(),tex.height()) };
            let at = r.center()-size*0.5;
            draw_texture_ex(tex,at.x,at.y,if enabled {WHITE} else {GRAY},DrawTextureParams{dest_size:Some(size),..Default::default()});
        }
        hover && is_mouse_button_pressed(MouseButton::Left)
    }
}

pub(super) async fn show(parts: &[Part], assets: &AssetCatalog, root: &Path, font: Option<&Font>) {
    let skin = Skin::load(assets);
    #[cfg(windows)]
    let renderer = crate::title_screen::tip_pango::Text::load(root, "Lato");
    let mut index = 0;
    while let Some(part) = parts.get(index) {
        let mut layers = HashMap::new();
        for layer in &part.layers {
            layers.entry(layer.file.as_str()).or_insert_with(|| {
                let path = assets.resolve_image(Path::new(&layer.file))?;
                let bytes = std::fs::read(path).ok()?;
                artwork::rgba(&bytes).ok().map(Artwork::new)
            });
        }
        let mut markers = HashMap::new();
        for file in part.markers.iter().map(|m| &m.file) {
            markers.entry(file.as_str()).or_insert_with(|| assets.resolve_image(Path::new(file))
                .and_then(|path| render::load_texture(&path, None)));
        }
        let mut first = 0;
        let mut previous_width = 0.;
        let mut wrapped = Vec::new();
        let mut block: Option<Texture2D> = None;
        loop {
            let scale = (screen_width() / 520.).min(screen_height() / 360.).clamp(0.01, 1.);
            let screen = vec2(screen_width(), screen_height()) / scale;
            let (mx, my) = mouse_position();
            let mouse = vec2(mx, my) / scale;
            set_camera(&dialog::setup_camera(screen.x, screen.y));
            clear_background(BLACK);
            draw_art(part, &layers, &markers, screen);
            if !part.title.is_empty() {
                dialog::text(font, &dialog::fit(font, part.title.clone(), screen.x - 40.0, 26),
                    20.0, 46.0, 26, WHITE);
            }
            let panel = part.placement.panel(screen, !part.title.is_empty());
            skin.panel(panel, part.placement, !part.title.is_empty());
            // The controls use 112 pixels plus their 20px side borders. The label
            // uses another 20px each side, and 3px separating its scrollbar.
            let width = (panel.w - 195.).max(1.);
            if previous_width != width {
                #[cfg(windows)]
                if let Some(renderer) = &renderer && !part.text.is_empty() {
                    let img = renderer.paragraph(&part.text, width as u16, 0);
                    let texture = Texture2D::from_image(&img);
                    texture.set_filter(FilterMode::Nearest);
                    block = Some(texture);
                }
                if block.is_none() { wrapped = lines(font, &plain_text(&part.text), width); }
                previous_width = width;
            }
            let count = ((panel.h - 40.) / 27.).floor().max(1.) as usize;
            let rows = block.as_ref().map_or(wrapped.len(), |t| (t.height()/27.).ceil() as usize);
            first = first.min(rows.saturating_sub(1));
            let shown = count.min(rows - first);
            if let Some(tex) = &block {
                let top = first as f32 * 27.;
                let source = Rect::new(0.,top,tex.width(),(tex.height()-top).min(count as f32*27.));
                draw_texture_ex(tex, 20., panel.y+20., WHITE,
                    DrawTextureParams { source:Some(source), dest_size:Some(source.size()), ..Default::default() });
            } else {
                for (row, line) in wrapped.iter().skip(first).take(count).enumerate() {
                    dialog::text(font, line, 20., panel.y + 38. + row as f32 * 27., 17, dialog::PALE);
                }
            }
            let skip_rect = Rect::new(panel.right() - 116.0, panel.bottom() - 53.0, 96.0, 33.0);
            let prior = Rect::new(skip_rect.x, skip_rect.y - 67.0, 40.0, 56.0);
            let advance = Rect::new(skip_rect.right() - 40.0, prior.y, 40.0, 56.0);
            let back = skin.control(0, prior, mouse, index > 0 || first > 0);
            let next = skin.control(1, advance, mouse, true);
            let skip = skin.control(2, skip_rect, mouse, true);
            let label_width = measure_text("Skip", font, 16, 1.0).width;
            dialog::text(font,"Skip",skip_rect.center().x-label_width*0.5,skip_rect.center().y+5.0,16,WHITE);
            let skip = skip || is_key_pressed(KeyCode::Escape);
            let back = back || is_key_pressed(KeyCode::Left) || is_key_pressed(KeyCode::PageUp);
            let next = next || [KeyCode::Right, KeyCode::PageDown, KeyCode::Enter, KeyCode::Space]
                .into_iter().any(is_key_pressed);
            set_default_camera();
            next_frame().await;
            if skip { return; }
            if back {
                if first > 0 { first = first.saturating_sub(count); }
                else if index > 0 { index -= 1; break; }
            } else if next {
                if first + shown < rows { first += shown; }
                else { index += 1; break; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn south_guard_opening_uses_the_integer_story_canvas_rectangle() {
        let config = engine_lite::wml::parse("background=story/fall.webp").unwrap();
        let layer = Layer::read(&config, true).unwrap();
        assert_eq!(layer.area(vec2(1024.,385.),vec2(1200.,820.)), Rect::new(0.,185.,1200.,451.));
    }

    #[test]
    fn narrative_keeps_the_authored_pango_span_color() {
        let scene = engine_lite::wml::parse("[story]\n[part]\nstory=\"<span color='#baac7d'>The king</span>\"\n[/part]\n[/story]").unwrap();
        assert_eq!(read(&scene, "")[0].text, "<span color='#baac7d'>The king</span>");
    }

    #[test]
    fn authored_story_text_layout_places_the_native_panel_below_the_title() {
        let scenario = engine_lite::wml::parse("[story]\n[part]\ntext_layout=middle\nstory=River\n[/part]\n[/story]").unwrap();
        let part = &read(&scenario, "River")[0];
        assert_eq!(part.placement.panel(vec2(1200.0, 800.0), true), Rect::new(0.0, 350.0, 1200.0, 200.0));
        assert_eq!(Placement::Bottom.panel(vec2(1000.0, 640.0), false).y, 440.0);
    }

    #[test]
    fn journey_maps_preserve_the_base_layer_and_static_route_coordinates() {
        let scenario = engine_lite::wml::parse(r#"[story]
[part]
[background_layer]
image=maps/background.webp
[/background_layer]
[background_layer]
base_layer=yes
image=maps/tsg.webp
scale_horizontally=no
scale_vertically=yes
[/background_layer]
[image]
file=misc/new-journey.png
x,y=1015,122
centered=yes
[/image]
[/part]
[/story]"#).unwrap();
        let parts = read(&scenario, "Born to the Banner");
        let map = &parts[0];
        assert_eq!(map.layers[0].file, "maps/background.webp");
        let base = &map.layers[1];
        assert!(base.base);
        assert_eq!(base.area(vec2(1200., 700.), vec2(800., 350.)), Rect::new(100., 0., 600., 350.));
        assert_eq!(map.markers[0].position, vec2(1015., 122.));
        assert!(map.markers[0].centered);
    }
}

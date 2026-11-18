//! Static campaign story parts shown before the board, including journey maps.
use std::{collections::HashMap, path::Path};

use engine_lite::wml::Node;
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;

use crate::{dialog, render};

pub(super) struct Part {
    pub(super) title: String,
    pub(super) text: String,
    layers: Vec<Layer>,
    markers: Vec<Marker>,
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
        let width = if self.scale_y && self.aspect { (original.x * screen.y / original.y).min(screen.x) }
            else if self.scale_x { screen.x } else { original.x };
        let height = if self.scale_x && self.aspect { (original.y * screen.x / original.x).min(screen.y) }
            else if self.scale_y { screen.y } else { original.y };
        Rect::new(((screen.x - width) / 2.).max(0.), ((screen.y - height) / 2.).max(0.), width, height)
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
            let text = plain_text(literal(node, "story").unwrap_or_default());
            if !text.is_empty() || !layers.is_empty() || !markers.is_empty() || !title.is_empty() {
                parts.push(Part { title, text, layers, markers });
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

fn draw_art(part: &Part, textures: &HashMap<&str, Option<Texture2D>>, screen: Vec2) {
    let mut origin = Vec2::ZERO;
    let mut scale = Vec2::ONE;
    for (index, layer) in part.layers.iter().enumerate() {
        let Some(Some(image)) = textures.get(layer.file.as_str()) else { continue };
        let size = vec2(image.width(), image.height());
        let rect = layer.area(size, screen);
        if index == 0 || layer.base {
            origin = rect.point();
            scale = rect.size() / size;
        }
        draw_texture_ex(image, rect.x, rect.y, WHITE,
            DrawTextureParams { dest_size: Some(rect.size()), ..Default::default() });
    }
    for marker in &part.markers {
        let Some(Some(image)) = textures.get(marker.file.as_str()) else { continue };
        let mut size = vec2(image.width(), image.height());
        if marker.resize { size *= scale; }
        let mut at = origin + marker.position * scale;
        if marker.centered { at -= size * 0.5; }
        draw_texture_ex(image, at.x, at.y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
    }
}

pub(super) async fn show(parts: &[Part], assets: &AssetCatalog, font: Option<&Font>) {
    let mut index = 0;
    while let Some(part) = parts.get(index) {
        let mut textures = HashMap::new();
        for file in part.layers.iter().map(|layer| &layer.file).chain(part.markers.iter().map(|m| &m.file)) {
            textures.entry(file.as_str()).or_insert_with(|| assets.resolve_image(Path::new(file))
                .and_then(|path| render::load_texture(&path, None)));
        }
        let mut first = 0;
        let mut previous_width = 0.;
        let mut wrapped = Vec::new();
        loop {
            let scale = (screen_width() / 520.).min(screen_height() / 360.).clamp(0.01, 1.);
            let screen = vec2(screen_width(), screen_height()) / scale;
            let (mx, my) = mouse_position();
            let mouse = vec2(mx, my) / scale;
            set_camera(&dialog::setup_camera(screen.x, screen.y));
            clear_background(BLACK);
            draw_art(part, &textures, screen);
            let transparent = Color::from_rgba(13, 12, 9, 230);
            if !part.title.is_empty() {
                draw_rectangle(0., 0., screen.x, 62., transparent);
                dialog::text(font, &dialog::fit(font, part.title.clone(), screen.x - 48., 29),
                    24., 42., 29, dialog::GOLD);
            }
            let width = (screen.x - 56.).min(980.);
            let left = (screen.x - width) * 0.5;
            if previous_width != width {
                wrapped = lines(font, &part.text, width);
                previous_width = width;
            }
            let count = ((screen.y * 0.4 / 25.) as usize).max(1);
            first = first.min(wrapped.len().saturating_sub(1));
            let shown = count.min(wrapped.len() - first);
            let top = screen.y - 65. - shown as f32 * 25.;
            draw_rectangle(0., top, screen.x, screen.y - top, transparent);
            for (row, line) in wrapped.iter().skip(first).take(count).enumerate() {
                dialog::text(font, line, left, top + 25. + row as f32 * 25., 19, dialog::PALE);
            }
            let button = |x, label: &str| {
                let rect = Rect::new(x, screen.y - 48., 91., 40.);
                dialog::button(rect, rect.contains(mouse), false);
                dialog::text(font, label, x + 13., rect.y + 27., 17, dialog::PALE);
                rect.contains(mouse) && is_mouse_button_pressed(MouseButton::Left)
            };
            let skip = button(left, "Skip");
            let back = (index > 0 || first > 0) && button(left + width - 192., "Back");
            let next = button(left + width - 91., if index + 1 == parts.len() && first + shown == wrapped.len() { "Begin" } else { "Next" });
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
                if first + shown < wrapped.len() { first += shown; }
                else { index += 1; break; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

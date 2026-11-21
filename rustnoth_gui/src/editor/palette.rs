use crate::{dialog, render};
use engine_lite::wml::{self, Node};
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;
use std::{collections::HashMap, path::Path};

pub(super) struct Brush {
    pub code: String,
    pub label: String,
    groups: Vec<String>,
    pub image: String,
    base: Option<String>,
}

impl Brush {
    pub fn paint(&self, original: &str, layer_only: bool) -> String {
        let (old_base, old_overlay) = original.split_once('^').unwrap_or((original, ""));
        if self.code.starts_with('^') {
            format!(
                "{}{}",
                if layer_only {
                    old_base
                } else {
                    self.base.as_deref().unwrap_or(old_base)
                },
                self.code
            )
        } else if layer_only {
            let base = self.code.split('^').next().unwrap_or(&self.code);
            if old_overlay.is_empty() {
                base.to_owned()
            } else {
                format!("{base}^{old_overlay}")
            }
        } else {
            self.code.clone()
        }
    }
}

const COLUMNS: usize = 4;
const STEP: f32 = 39.0;

struct Group {
    id: String,
    label: String,
    icon: String,
    core: bool,
    entries: Vec<usize>,
}

pub(super) struct Palette {
    pub brushes: Vec<Brush>,
    groups: Vec<Group>,
    selected_group: usize,
    pub fg: usize,
    pub bg: usize,
    first: usize,
    menu_start: Option<usize>,
}

fn choices(cfg: &Node) -> Vec<Brush> {
    let names: HashMap<_, _> = cfg.children.iter()
        .filter(|(tag, _)| tag == "terrain_type")
        .filter_map(|(_, node)| Some((node.attribute("string")?, node.attribute("name")?)))
        .collect();
    let mut brushes: Vec<_> = cfg.children.iter()
        .filter(|(tag, _)| tag == "terrain_type")
        .filter_map(|(_, node)| {
            let code = node.attribute("string")?;
            let image = node.attribute("editor_image").filter(|s| !s.is_empty())
                .or_else(|| node.attribute("symbol_image"))?;
            let hidden = ["hide_in_editor", "hidden"].iter()
                .any(|key| matches!(node.attribute(key), Some("true" | "yes" | "1")));
            let groups = node.attribute("editor_group").unwrap_or_default().split(',')
                .map(str::trim).filter(|g| !hidden && !g.is_empty()).map(str::to_owned).collect();
            let name = node.attribute("name").unwrap_or(code);
            let editor_name = node.attribute("editor_name").unwrap_or(name);
            let mut label = editor_name.to_owned();
            if editor_name != name {
                label.push('/');
                label.push_str(name);
            }
            let aliases: Vec<_> = node.attribute("aliasof").unwrap_or_default()
                .split(',').map(str::trim).filter(|&alias| alias != code)
                .filter_map(|alias| names.get(alias).copied()).collect();
            if !aliases.is_empty() {
                label.push_str(&format!(" ({})", aliases.join(", ")));
            }
            Some(Brush {
                code: code.into(),
                groups,
                label,
                base: node.attribute("default_base").filter(|s| !s.is_empty()).map(str::to_owned),
                image: format!("terrain/{image}.png"),
            })
        }).collect();
    brushes.sort_by_key(|b| matches!(b.code.as_str(), "_s" | "_f"));
    brushes
}

fn groups(cfg: &Node, installed: &Node, brushes: &[Brush]) -> Vec<Group> {
    let mut groups = Vec::<Group>::new();
    for tag in ["terrain_group", "editor_group"] {
        for (_, node) in cfg.children.iter().chain(&installed.children).filter(|(t, _)| t == tag) {
            let Some(id) = node.attribute("id") else { continue };
            if groups.iter().any(|g| g.id == id) { continue }
            let icon = node.attribute("icon").unwrap_or_default();
            groups.push(Group {
                id: id.to_owned(),
                label: node.attribute("name").unwrap_or(id).into(),
                icon: if tag == "editor_group" { format!("icons/terrain/terrain_{icon}") } else { icon.into() },
                core: matches!(node.attribute("core"), Some("true" | "yes" | "1")),
                entries: Vec::new(),
            });
        }
    }
    // Some standalone core files omit the editor-only group definitions.
    if groups.is_empty() {
        for id in brushes.iter().flat_map(|b| &b.groups) {
            if !groups.iter().any(|g| &g.id == id) {
                groups.push(Group {
                    id: id.clone(),
                    label: id.clone(),
                    icon: "icons/terrain/terrain_group_all".into(),
                    core: true,
                    entries: Vec::new(),
                });
            }
        }
    }
    if !groups.iter().any(|g| g.id == "all") {
        groups.insert(0, Group {
            id: "all".into(),
            label: "all".into(),
            icon: "icons/terrain/terrain_group_all".into(),
            core: false,
            entries: Vec::new(),
        });
    }
    for (i, brush) in brushes.iter().enumerate() {
        let core = groups.iter().any(|g| g.core && brush.groups.contains(&g.id));
        for group in &mut groups {
            if (group.id == "all" && core) || brush.groups.contains(&group.id) {
                group.entries.push(i);
            }
        }
    }
    groups
}

pub(super) struct Art {
    cache: HashMap<String, Option<Texture2D>>,
}

impl Art {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }
    pub fn image(&mut self, assets: &AssetCatalog, key: &str, rect: Rect) {
        self.tinted(assets, key, rect, WHITE);
    }

    fn texture(&mut self, assets: &AssetCatalog, key: &str) -> Option<&Texture2D> {
        self.cache.entry(key.into()).or_insert_with(|| {
            if let Some(name) = key.strip_suffix("~GS()") {
                let path = assets.resolve_image(Path::new(name))?;
                let mut image = crate::artwork::image(&std::fs::read(path).ok()?).ok()?;
                for px in image.get_image_data_mut().iter_mut().filter(|px| px[3] != 0) {
                    let gray = ((77 * u16::from(px[0]) + 150 * u16::from(px[1])
                        + 29 * u16::from(px[2])) / 256) as u8;
                    px[..3].fill(gray);
                }
                return Some(Texture2D::from_image(&image));
            }
            assets
                .resolve_image(Path::new(key))
                .filter(|p| p.is_file())
                .and_then(|path| render::load_texture(&path, None))
        }).as_ref()
    }

    // Classic theme panels repeat their original bitmap and clip the last tile.
    pub fn panel(&mut self, assets: &AssetCatalog, key: &str, rect: Rect) {
        let Some(image) = self.texture(assets, key) else { return };
        let mut y = rect.y;
        while y < rect.bottom() {
            let mut x = rect.x;
            while x < rect.right() {
                let source = Rect::new(0., 0., image.width().min(rect.right() - x),
                    image.height().min(rect.bottom() - y));
                draw_texture_ex(image, x, y, WHITE,
                    DrawTextureParams { source: Some(source), dest_size: Some(source.size()), ..Default::default() });
                x += image.width();
            }
            y += image.height();
        }
    }

    pub fn tinted(&mut self, assets: &AssetCatalog, key: &str, rect: Rect, tint: Color) {
        if let Some(image) = self.texture(assets, key) {
            draw_texture_ex(
                image,
                rect.x,
                rect.y,
                tint,
                DrawTextureParams {
                    dest_size: Some(rect.size()),
                    ..Default::default()
                },
            );
        }
    }
}

impl Palette {
    pub fn load(cfg: &Node, root: &Path) -> Result<Self, &'static str> {
        let installed = std::fs::read(root.join("data/core/editor/terrain-groups.cfg"))
            .ok().and_then(|bytes| wml::parse(bytes).ok()).unwrap_or_default();
        Self::new(cfg, &installed)
    }

    pub(super) fn new(cfg: &Node, installed: &Node) -> Result<Self, &'static str> {
        let brushes = choices(cfg);
        let groups = groups(cfg, installed, &brushes);
        let first_entry = groups.iter().flat_map(|g| &g.entries).next().copied()
            .ok_or("No editor terrain definitions were found in the core data.")?;
        let fg = brushes.iter().position(|b| b.code == "Mm").unwrap_or(first_entry);
        let bg = brushes.iter().position(|b| b.code == "Gg").unwrap_or(fg);
        let selected_group = groups.iter().position(|g| g.id == "all").unwrap_or(0);
        Ok(Self {
            brushes,
            groups,
            fg,
            bg,
            selected_group,
            first: 0,
            menu_start: None,
        })
    }

    pub fn group_menu_open(&self) -> bool {
        self.menu_start.is_some()
    }

    pub fn help(&self) -> [String; 2] {
        let foreground = &self.brushes[self.fg];
        let layer = if foreground.code.starts_with('^') { "overlay" } else { "base" };
        [
            format!("Left-click: {} | Right-click: {}", foreground.label, self.brushes[self.bg].label),
            format!("Shift + left-click: paint {layer} layer only | Ctrl + click: copy terrain"),
        ]
    }

    fn tile(&self, i: usize, bounds: Rect, art: &mut Art, assets: &AssetCatalog) {
        let b = &self.brushes[i];
        if let Some(base) = b
            .base
            .as_ref()
            .and_then(|base| self.brushes.iter().find(|x| &x.code == base))
        {
            art.image(assets, &base.image, bounds);
        }
        art.image(assets, &b.image, bounds);
    }

    pub fn sample(&mut self, code: &str, foreground: bool) {
        let i = if let Some(i) = self.brushes.iter().position(|b| b.code == code) {
            i
        } else {
            // Ctrl-click can pick a base/overlay combination absent from the palette.
            let Some((base, _)) = code.split_once('^') else { return; };
            let Some(overlay) = self.brushes.iter().find(|b| b.code == code[base.len()..]) else { return; };
            let Some(ground) = self.brushes.iter().find(|b| b.code == base) else { return; };
            let combined = Brush {
                code: code.into(),
                groups: Vec::new(),
                base: Some(base.into()),
                label: format!("{} / {}", ground.label, overlay.label),
                image: overlay.image.clone(),
            };
            self.brushes.push(combined);
            self.brushes.len() - 1
        };
        if foreground {
            self.fg = i;
        } else {
            self.bg = i;
        }
    }

    pub fn panel(
        &mut self,
        bounds: Rect,
        compact: bool,
        point: Vec2,
        assets: &AssetCatalog,
        art: &mut Art,
        font: Option<&Font>,
    ) {
        let top = bounds.y + 35.0;
        let library_y = top + 5.0;
        let visible = ((bounds.bottom() - library_y) / STEP).floor().max(0.0) as usize;
        let group = &self.groups[self.selected_group];
        let last = group.entries.len().div_ceil(COLUMNS).saturating_sub(visible);
        let click = is_mouse_button_pressed(MouseButton::Left);
        let wheel = mouse_wheel().1;
        let group_x = if compact { 12.0 } else { 14.0 };
        let swap_x = if compact { 81.0 } else { 78.0 };
        let swap_w = if compact { 25.0 } else { 30.0 };
        let controls = [
            (group_x, 30.0, format!("{}_30", group.icon), true),
            (swap_x, swap_w, format!("icons/action/editor-terrain-palette-swap_{swap_w:.0}"), true),
            (swap_x + swap_w + 7.0, 25.0, "icons/arrows/arrows_ornate_up_25".into(), self.first > 0),
            (swap_x + swap_w + 39.0, 25.0, "icons/arrows/arrows_ornate_down_25".into(), self.first < last),
        ];
        for (i, (x, size, icon, enabled)) in controls.into_iter().enumerate() {
            let rect = Rect::new(bounds.x + x, bounds.y, size, size);
            let hot = rect.contains(point);
            let suffix = if hot && enabled { "-active" } else { "" };
            art.image(assets, &format!("buttons/button_square/button_square_{size:.0}{suffix}.png"), rect);
            art.image(assets, &format!("{icon}.png"), rect);
            if !enabled {
                draw_rectangle(rect.x + 1.0, rect.y + 1.0, rect.w - 2.0, rect.h - 2.0,
                    Color::new(0.15, 0.15, 0.15, 0.60));
            }
            if hot && click && enabled {
                match i {
                    0 => self.menu_start = if self.menu_start.is_none() { Some(self.selected_group) } else { None },
                    1 => std::mem::swap(&mut self.fg, &mut self.bg),
                    2 => self.first = self.first.saturating_sub(1),
                    _ => self.first += 1,
                }
            }
        }
        art.panel(assets, "themes/editor/classic/palette_top.png", Rect::new(bounds.x, top, bounds.w, 45.0));
        art.panel(assets, "themes/editor/classic/palette.png", Rect::new(bounds.x, top + 45.0, bounds.w, (bounds.bottom() - top - 45.0).max(0.0)));

        if let Some(start) = self.menu_start {
            let rows = (((bounds.bottom() - library_y) / 24.0).floor() as usize).max(1);
            let max_start = self.groups.len().saturating_sub(rows);
            let mut start = start.min(max_start);
            if bounds.contains(point) && wheel != 0.0 {
                start = start.saturating_add_signed(if wheel > 0.0 { -1 } else { 1 }).min(max_start);
            }
            self.menu_start = Some(start);
            for (i, group) in self.groups.iter().enumerate().skip(start).take(rows) {
                let rect = Rect::new(bounds.x + 14.0, library_y + (i - start) as f32 * 24.0, bounds.w - 25.0, 24.0);
                let hot = rect.contains(point);
                let color = if hot { Color::from_rgba(44, 65, 71, 255) } else { Color::from_rgba(31, 32, 35, 255) };
                draw_rectangle(rect.x, rect.y, rect.w, rect.h, color);
                art.image(assets, &format!("{}_30.png", group.icon), Rect::new(rect.x + 1.0, rect.y + 1.0, 22.0, 22.0));
                dialog::text(font, &group.label, rect.x + 27.0, rect.y + 17.0, 15, WHITE);
                if hot && click {
                    self.selected_group = i;
                    self.first = 0;
                    self.menu_start = None;
                }
            }
            if (click && !bounds.contains(point)) || is_key_pressed(KeyCode::Escape) {
                self.menu_start = None;
            }
            return;
        }
        if bounds.contains(point) && wheel != 0.0 {
            self.first = self.first.saturating_add_signed(if wheel > 0.0 { -1 } else { 1 });
        }
        self.first = self.first.min(last);
        let entries = &self.groups[self.selected_group].entries;
        for (n, &i) in entries.iter().skip(self.first * COLUMNS).take(visible * COLUMNS).enumerate() {
            let r = Rect::new(bounds.x + 15.0 + (n % COLUMNS) as f32 * STEP,
                library_y + (n / COLUMNS) as f32 * STEP, 38.0, 38.0);
            let hot = r.contains(point);
            if hot {
                if click { self.fg = i; }
                if is_mouse_button_pressed(MouseButton::Right) { self.bg = i; }
            }
            let selected = match (i == self.fg, i == self.bg) {
                (true, true) => "both",
                (true, false) => "up",
                (false, true) => "down",
                _ => "",
            };
            let active = if hot || !selected.is_empty() { "-active" } else { "" };
            let prefix = "buttons/button_selectable/button_selectable_38_";
            art.image(assets, &format!("{prefix}base{active}.png"), r);
            self.tile(i, Rect::new(r.x + 1.0, r.y + 1.0, 36.0, 36.0), art, assets);
            if !selected.is_empty() {
                let border = if hot { "active-pressed" } else { "pressed" };
                art.image(assets, &format!("{prefix}border-{border}-{selected}.png"), r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_single_layer_brush_keeps_the_other_layer_and_full_overlay_uses_its_default_base() {
        let b = Brush {
            code: "^Wkf".into(),
            base: Some("Ww".into()),
            groups: Vec::new(),
            label: String::new(),
            image: String::new(),
        };
        assert_eq!(b.paint("Wo^Ewl", false), "Ww^Wkf");
        assert_eq!(b.paint("Wo^Ewl", true), "Wo^Wkf");
        let grass = Brush {
            code: "Gg".into(),
            ..b
        };
        assert_eq!(grass.paint("Gs^Fds", true), "Gg^Fds");
        assert_eq!(grass.paint("Gs^Fds", false), "Gg");
    }

    #[test]
    fn native_palette_opens_on_core_terrains_and_keeps_group_and_position_when_sampling() {
        let rows = [
            ("Ww", "Water", "water", ""), ("Gg", "Grassland", "flat", ""),
            ("Mm", "Mountains", "rough", ""), ("^Wkf", "Kelp", "extra,water", ""),
            ("Dd", "Custom", "extra", ""), ("Xv", "Hidden", "rough", "hidden=yes"),
        ];
        let mut cfg = String::new();
        for (code, name, group, flags) in rows {
            cfg.push_str(&format!("[terrain_type]\nstring={code}\nname={name}\neditor_group={group}\nsymbol_image=tile\n{flags}\n[/terrain_type]\n"));
        }
        for (id, core) in [("water", "yes"), ("flat", "yes"), ("rough", "yes"), ("extra", "no")] {
            cfg.push_str(&format!("[editor_group]\nid={id}\ncore={core}\n[/editor_group]\n"));
        }
        let mut palette = Palette::new(&wml::parse(cfg).unwrap(), &Node::default()).unwrap();
        assert_eq!(palette.brushes[palette.fg].code, "Mm");
        assert_eq!(palette.brushes[palette.bg].code, "Gg");
        let group = &palette.groups[palette.selected_group];
        assert_eq!(group.id, "all");
        let codes: Vec<_> = group.entries.iter().map(|&i| palette.brushes[i].code.as_str()).collect();
        assert_eq!(codes, ["Ww", "Gg", "Mm", "^Wkf"]);
        palette.first = 1;
        palette.sample("Mm^Wkf", false);
        assert_eq!(palette.brushes[palette.bg].paint("Gg", false), "Mm^Wkf");
        assert_eq!(palette.brushes[palette.bg].paint("Gg", true), "Mm");
        assert_eq!(palette.groups[palette.selected_group].id, "all");
        assert_eq!(palette.first, 1);
        assert_eq!(palette.groups[palette.selected_group].entries.len(), 4);
    }
}

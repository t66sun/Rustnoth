use super::{Art, AssetCatalog, Observation};
use crate::dialog;
use macroquad::prelude::*;
use std::{collections::HashMap, path::Path};

pub(super) struct Villages {
    count: usize,
    #[cfg(windows)]
    renderer: Option<crate::title_screen::tip_pango::Text>,
    #[cfg(windows)]
    text: Option<Texture2D>,
}

fn count(view: &Observation, assets: &AssetCatalog) -> usize {
    let mut kinds = HashMap::new();
    view.tiles.iter().filter(|tile| {
        (0..view.width).contains(&tile.position.x) && (0..view.height).contains(&tile.position.y)
            && *kinds.entry(&tile.terrain).or_insert_with(|| assets.minimap_terrain(&tile.terrain).1)
    }).count()
}

impl Villages {
    pub fn new(root: &Path) -> Self {
        Self { count: 0,
            #[cfg(windows)] renderer: crate::title_screen::tip_pango::Text::load(root, "Lato"),
            #[cfg(windows)] text: None }
    }

    pub fn update(&mut self, view: &Observation, assets: &AssetCatalog) {
        let next = count(view, assets);
        #[cfg(windows)]
        if (next != self.count || self.text.is_none()) && let Some(renderer) = &self.renderer {
            self.text = Some(Texture2D::from_image(&renderer.button(&next.to_string(), 15, [221;3])));
        }
        self.count = next;
    }

    pub fn draw(&self, width: f32, art: &mut Art, assets: &AssetCatalog, font: Option<&Font>) {
        art.image(assets, "themes/villages.png", Rect::new(610.,4.,16.,16.));
        if width >= 1024. { art.image(assets,"themes/units.png",Rect::new(685.,4.,16.,16.)); }
        #[cfg(windows)]
        if let Some(tex) = &self.text { draw_texture(tex,630.,4.,WHITE); return; }
        dialog::text(font, &self.count.to_string(), 630., 18., 15, Color::from_rgba(221,221,221,255));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::map::ScenarioMap;
    #[test]
    fn editor_counts_income_terrain_on_the_map_including_village_overlays() {
        let core=b"[terrain_type]\nstring=Gg\nsymbol_image=g\n[/terrain_type]\n[terrain_type]\nstring=^Vh\ngives_income=yes\nsymbol_image=v\n[/terrain_type]";
        let assets=AssetCatalog::parse(Path::new(""),core,b"").unwrap();
        let map=ScenarioMap::parse("Gg^Vh,Gg,Gg,Gg\nGg,Gg^Vh,Gg,Gg\nGg,Gg,Gg^Vh,Gg\nGg,Gg,Gg,Gg").unwrap();
        let view=super::super::render::map_observation(&map);
        assert_eq!(count(&view,&assets),2);
    }
}

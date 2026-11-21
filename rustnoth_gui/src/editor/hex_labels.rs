use super::{Command, Hex};
use crate::{dialog, display_preferences::HexReport};
use macroquad::prelude::*;
use std::{io, path::Path};

pub(super) struct Labels {
    pub coordinates: bool,
    pub terrain: bool,
    #[cfg(windows)]
    renderer: Option<crate::title_screen::tip_pango::Text>,
    #[cfg(windows)]
    cached: std::collections::HashMap<String, (Texture2D, bool)>,
}

impl Labels {
    pub fn new(root: &Path) -> io::Result<Self> {
        Ok(Self {
            coordinates: HexReport::Coordinates.load()?, terrain: HexReport::Terrain.load()?,
            #[cfg(windows)]
            renderer: crate::title_screen::tip_pango::Text::load(root, "Lato"),
            #[cfg(windows)]
            cached: Default::default(),
        })
    }

    pub fn toggle(&mut self, command: Command) -> io::Result<()> {
        let (setting, value) = match command {
            Command::Coordinates => (HexReport::Coordinates, &mut self.coordinates),
            Command::TerrainCodes => (HexReport::Terrain, &mut self.terrain),
            _ => return Ok(()),
        };
        setting.save(!*value)?;
        *value = !*value;
        Ok(())
    }

    pub fn begin(&mut self) {
        // Keep glyph textures only while their labels are visible.
        #[cfg(windows)]
        self.cached.retain(|_, (_, used)| std::mem::replace(used, false));
    }

    pub fn draw(&mut self, hex: Hex, code: &str, center: Vec2, font: Option<&Font>) {
        if !self.coordinates && !self.terrain { return; }
        let text = report(hex, code, self.coordinates, self.terrain);
        #[cfg(windows)]
        if let Some(renderer) = &self.renderer {
            let (tex, used) = self.cached.entry(text.clone()).or_insert_with(|| {
                let tex = Texture2D::from_image(&renderer.hex_report(&text));
                tex.set_filter(FilterMode::Nearest);
                (tex, false)
            });
            *used = true;
            let pos = center.floor() - (tex.size() / 2.).floor();
            draw_rectangle(pos.x-3., pos.y-3., tex.width()+6., tex.height()+6., Color::from_rgba(0,0,0,170));
            draw_texture(tex, pos.x, pos.y, WHITE);
            return;
        }
        let w = text.lines().map(|s| measure_text(s,font,13,1.).width).fold(0., f32::max);
        let h = 15. * text.lines().count() as f32;
        draw_rectangle(center.x-w/2.-3., center.y-h/2.-3., w+6., h+6., Color::from_rgba(0,0,0,170));
        for (row, line) in text.lines().enumerate() {
            let metrics = measure_text(line,font,13,1.);
            dialog::text(font,line,center.x-metrics.width/2.,center.y-h/2.+row as f32*15.+13.,13,
                Color::from_rgba(221,221,221,255));
        }
    }
}

fn report(hex: Hex, code: &str, coordinates: bool, terrain: bool) -> String {
    if !coordinates { return code.to_owned(); }
    let mut value = format!("{},{}", hex.x+1, hex.y+1);
    if terrain { value.push('\n'); value.push_str(code); }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_use_wml_coordinates_and_preserve_composite_terrain_codes() {
        let hex = Hex{x:3,y:6};
        assert_eq!(report(hex,"Gg^Vh",true,true),"4,7\nGg^Vh");
        assert_eq!(report(hex,"Wo",false,true),"Wo");
        assert_eq!(report(hex,"Wo",true,false),"4,7");
    }
}

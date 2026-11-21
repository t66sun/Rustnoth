//! GUI2's load-game minimap composes 24px terrain textures, then scales the result.
use super::*;

pub(crate) fn save_preview(assets: &AssetCatalog, view: &Observation, size: f32) -> Option<Texture2D> {
    let frame = Geometry::new(view.width, view.height, Viewport { x: 0., y: 0., width: size, height: size })?;
    let (width, height) = (frame.bounds.w as u32, frame.bounds.h as u32);
    let raw = [view.width as u32 * 18, view.height as u32 * 24];
    let xs = nearest(raw[0], width);
    let ys = nearest(raw[1], height);
    let mut out = RgbaImage::new(width, height);
    let mut cache = HashMap::new();
    let mut villages = Vec::new();
    for tile in &view.tiles {
        let (symbol, village) = cache.entry(&tile.terrain).or_insert_with(|| {
            let (layers, village) = assets.minimap_terrain(&tile.terrain);
            let mut tile = RgbaImage::new(SAMPLE, SAMPLE);
            for path in layers {
                if let Ok(source) = image::open(path) {
                    let layer = imageops::resize(&source.to_rgba8(), SAMPLE, SAMPLE, imageops::FilterType::Nearest);
                    imageops::overlay(&mut tile, &layer, 0, 0);
                }
            }
            (tile, village)
        });
        let left = tile.position.x * 18 - 6;
        let top = tile.position.y * 24 + (tile.position.x.rem_euclid(2) - 1) * 12;
        let range = |coordinates: &[i32], start, len| coordinates.partition_point(|&p| p < start)..coordinates.partition_point(|&p| p < start + len);
        let rows = range(&ys, top, 24);
        for y in rows.clone() {
            for x in range(&xs, left, 24) {
                let mut pixel = *symbol.get_pixel((xs[x]-left) as u32, (ys[y]-top) as u32);
                if !tile.visible {
                    for channel in &mut pixel.0[..3] { *channel = channel.saturating_sub(50); }
                }
                out.get_pixel_mut(x as u32, y as u32).blend(&pixel);
            }
        }
        if *village { villages.push((range(&xs, left, 18), rows)); }
    }
    // The preview widget has no gameboard or unit map. Villages are neutral.
    for (cols, rows) in villages {
        for y in rows {
            for x in cols.clone() { out.put_pixel(x as u32, y as u32, NEUTRAL_VILLAGE); }
        }
    }
    let texture = Texture2D::from_rgba8(width as u16, height as u16, out.as_raw());
    texture.set_filter(FilterMode::Nearest);
    Some(texture)
}

fn nearest(source: u32, destination: u32) -> Vec<i32> {
    (0..u64::from(destination)).map(|p| (((2*p+1)*u64::from(source))/(2*u64::from(destination))) as i32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_map_samples_the_centers_of_both_scaled_canvases() {
        assert_eq!(nearest(72, 24)[..4], [1, 4, 7, 10]);
        assert_eq!(nearest(7, 3), [1, 3, 5]);
    }
}

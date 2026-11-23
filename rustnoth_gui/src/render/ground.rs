//! Stock NEW:BASE and NEW:BASE_P rules for the ground under terrain overlays.
use super::{AssetCatalog, Hex, HashMap, Path, PathBuf, Texture2D, get_texture};
use std::fs;

#[derive(Default)]
pub(super) struct Ground {
    rules: HashMap<String, (String, Option<u32>)>,
    loaded: HashMap<String, Option<Variants>>,
}

struct Variants {
    textures: Vec<Texture2D>,
    varied: u32,
    plain: u32,
    probability: Option<u32>,
}

pub(super) fn fingerprint(name: &str) -> u32 {
    name.bytes().fold(0u32, |value, byte| value.rotate_left(9) ^ u32::from(byte))
}

pub(super) fn noise(hex: Hex, fingerprint: u32) -> u32 {
    let x = (hex.x as u32).wrapping_add(92_872_973) ^ 918_273;
    let y = (hex.y as u32).wrapping_add(1_672_517) ^ 128_123;
    let seed = fingerprint.wrapping_add(127_390) ^ 13_923_787;
    let value = x.wrapping_add(1).wrapping_mul(y.wrapping_add(1))
        .wrapping_mul(seed.wrapping_add(1)).wrapping_sub(1);
    value.wrapping_mul(value)
}

fn variation(hex: Hex, count: usize, varied: u32, plain: u32, probability: Option<u32>) -> usize {
    if probability.is_some_and(|p| noise(hex, plain) % 100 <= p) { return 0; }
    (noise(hex, varied) / 7919) as usize % count
}

fn base_code(terrain: &str) -> &str {
    terrain.split_once('^').map_or(terrain, |(base, _)| base)
}

impl Ground {
    pub(super) fn new(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("data/core/terrain-graphics.cfg")).unwrap_or_default();
        Self { rules: ground_rules(&source), loaded: HashMap::new() }
    }

    pub(super) fn load(&mut self, terrain: &str, catalog: &AssetCatalog, cache: &mut HashMap<PathBuf, Option<Texture2D>>) {
        let code = base_code(terrain);
        if self.loaded.contains_key(code) { return; }
        let variants = self.rules.get(code).and_then(|(stem, probability)| {
            let mut textures = Vec::new();
            for suffix in ["", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"] {
                let name = format!("terrain/{stem}{suffix}.png");
                let Some(path) = catalog.resolve_image(Path::new(&name)).filter(|p| p.is_file()) else { break };
                let Some(texture) = get_texture(cache, path) else { break };
                textures.push(texture);
            }
            (!textures.is_empty()).then(|| Variants {
                textures,
                varied: fingerprint(&format!("{stem}@V.png")),
                plain: fingerprint(&format!("{stem}.png")),
                probability: *probability,
            })
        });
        self.loaded.insert(code.to_owned(), variants);
    }

    pub(super) fn texture(&self, terrain: &str, hex: Hex) -> Option<&Texture2D> {
        let set = self.loaded.get(base_code(terrain))?.as_ref()?;
        set.textures.get(variation(hex, set.textures.len(), set.varied, set.plain, set.probability))
    }

    pub(super) fn texture_extent(&self) -> super::Vec2 {
        super::texture_extent(self.loaded.values().flatten().flat_map(|variants| &variants.textures))
    }
}

fn ground_rules(source: &str) -> HashMap<String, (String, Option<u32>)> {
    let mut rules = HashMap::new();
    for line in source.lines() {
        let definition = line.split('#').next().unwrap_or("").trim();
        let Some(args) = definition.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else { continue };
        let fields: Vec<_> = args.split_whitespace().collect();
        let (codes, stem, probability) = match fields.as_slice() {
            ["NEW:BASE", codes, stem] => (*codes, *stem, None),
            ["NEW:BASE_P", codes, chance, stem] => (*codes, *stem, chance.parse().ok()),
            _ => continue,
        };
        for code in codes.trim_matches(['(', ')']).split(',') {
            // Mountain and castle tiles also require multi-hex structures above their floors.
            if !code.starts_with(['G', 'H', 'R', 'D', 'U']) || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
                continue;
            }
            let rule = rules.entry(code.to_owned()).or_insert_with(|| (stem.to_owned(), None));
            if rule.0 == stem && probability.is_some() { rule.1 = probability; }
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_fixed_grass_and_location_variants_have_distinct_native_seeds() {
        let rules = ground_rules("{NEW:BASE_P Gg 20 grass/green}\n{NEW:BASE Gg grass/green}\n{NEW:BASE (Rb,Exos) flat/dirt-dark}\n");
        assert_eq!(rules["Gg"], ("grass/green".into(), Some(20)));
        assert_eq!(rules["Rb"], ("flat/dirt-dark".into(), None));
        let plain = fingerprint("grass/green.png");
        let varied = fingerprint("grass/green@V.png");
        let indices: Vec<_> = (0..10).map(|x| variation(Hex { x, y: 3 }, 8, varied, plain, Some(20))).collect();
        assert_eq!(indices, [6, 3, 3, 6, 0, 5, 7, 6, 0, 2]);
        assert_eq!(base_code("Gg^Fms"), "Gg");
    }
}

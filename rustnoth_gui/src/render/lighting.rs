//! Wesnoth's additive terrain light and directional regional-time masks.
use std::{cell::Cell, collections::HashMap, fs, path::Path};

use engine_lite::{hex::{Direction, Hex}, observation::{Observation, ObservedTile}};
use macroquad::{miniquad::{BlendFactor, BlendState, BlendValue, Equation}, prelude::*};

type Shift = [i32; 3];
const EDGE_MASKS: [&str; 18] = [
    "concave-2-tr", "concave-2-r", "concave-2-br", "concave-2-bl", "concave-2-l", "concave-2-tl",
    "convex-br-bl", "convex-bl-l", "convex-l-tl", "convex-tl-tr", "convex-tr-r", "convex-r-br",
    "convex-l-bl", "convex-tl-l", "convex-tr-tl", "convex-r-tr", "convex-br-r", "convex-bl-br",
];

/// Native light_adjust stores half-channel signed bytes. Flat terrain doubles them again.
fn terrain_shift(color: Shift) -> Shift {
    color.map(|value| (value / 2).clamp(-128, 127) * 2)
}

fn normalized(rgb: Shift) -> Vec3 {
    Vec3::from_array(rgb.map(|value| value.clamp(-256, 255) as f32 / 255.0))
}

fn mask_atlas(directory: &Path) -> Texture2D {
    let mut atlas = Image::gen_image_color(432, 216, BLANK);
    for (i, name) in EDGE_MASKS.iter().enumerate() {
        let path = directory.join(format!("light-{name}.png"));
        let Some(mask) = fs::read(path).ok().and_then(|bytes| Image::from_file_with_format(&bytes, None).ok()) else {
            continue;
        };
        if (mask.width, mask.height) != (72, 72) { continue; }
        for (y, row) in mask.bytes.chunks_exact(72 * 4).enumerate() {
            let offset = ((i / 6 * 72 + y) * 432 + i % 6 * 72) * 4;
            atlas.bytes[offset..offset + row.len()].copy_from_slice(row);
        }
    }
    let texture = Texture2D::from_image(&atlas);
    texture.set_filter(FilterMode::Linear);
    texture
}

pub(super) struct Lighting {
    material: Material,
    terrain: TerrainLighting,
    // Flat neighboring hexes with the same schedule stay in the same GPU batch.
    active_flat: Cell<Option<Shift>>,
}

enum TerrainLighting {
    Uniform(Shift),
    Regional {
        colors: HashMap<Hex, Shift>,
        edges: HashMap<Hex, [Shift; 6]>,
    },
}

impl Default for TerrainLighting {
    fn default() -> Self {
        Self::Uniform([0; 3])
    }
}

impl TerrainLighting {
    fn update(&mut self, view: &Observation) {
        let first = view.tiles.first().map_or([0; 3], |tile| tile.time_color);
        if view.tiles.iter().all(|tile| tile.time_color == first) {
            *self = Self::Uniform(terrain_shift(first));
            return;
        }

        let (mut colors, mut edges) = match std::mem::take(self) {
            Self::Uniform(_) => (HashMap::new(), HashMap::new()),
            Self::Regional { colors, edges } => (colors, edges),
        };
        colors.clear();
        colors.extend(view.tiles.iter().map(|tile| (tile.position, terrain_shift(tile.time_color))));
        edges.clear();
        for (&hex, &color) in &colors {
            let neighbors = Direction::CLOCKWISE.map(|dir| hex.neighbor(dir)
                .and_then(|pos| colors.get(&pos)).copied().unwrap_or(color));
            if neighbors.iter().any(|other| *other != color) {
                edges.insert(hex, neighbors);
            }
        }
        *self = Self::Regional { colors, edges };
    }

    fn shade(&self, hex: Hex) -> (Shift, Option<&[Shift; 6]>) {
        match self {
            Self::Uniform(color) => (*color, None),
            Self::Regional { colors, edges } => (colors[&hex], edges.get(&hex)),
        }
    }
}

impl Lighting {
    pub(super) fn new(directory: &Path) -> Result<Self, macroquad::Error> {
        let material = load_material(ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT }, MaterialParams {
            pipeline_params: PipelineParams {
                color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha))),
                ..Default::default()
            },
            uniforms: vec![UniformDesc::new("Shift", UniformType::Float3),
                UniformDesc::new("HexBox", UniformType::Float3),
                UniformDesc::new("Adjacent", UniformType::Float3).array(7)],
            textures: vec!["Masks".into()],
        })?;
        material.set_texture("Masks", mask_atlas(directory));
        Ok(Self { material, terrain: TerrainLighting::default(), active_flat: Cell::new(Some([0; 3])) })
    }

    pub(super) fn update(&mut self, view: &Observation) {
        self.terrain.update(view);
    }

    pub(super) fn flat(&self, color: Shift) {
        if self.active_flat.get() == Some(color) { return; }
        if color == [0; 3] {
            gl_use_default_material();
        } else {
            gl_use_material(&self.material);
            self.material.set_uniform("Shift", normalized(color));
            self.material.set_uniform("HexBox", Vec3::ZERO);
        }
        self.active_flat.set(Some(color));
    }

    pub(super) fn terrain(&self, hex: Hex, left: f32, top: f32, size: f32) {
        let (color, neighbors) = self.terrain.shade(hex);
        if let Some(neighbors) = neighbors {
            self.active_flat.set(None);
            gl_use_material(&self.material);
            self.material.set_uniform("Shift", normalized(color));
            self.material.set_uniform("HexBox", vec3(left, top, size));
            let adjacent: [Vec3; 7] = std::array::from_fn(|i| normalized(neighbors[i % 6]));
            self.material.set_uniform_array("Adjacent", &adjacent);
        } else {
            self.flat(color);
        }
    }

    pub(super) fn item(&self, tile: Option<&ObservedTile>) {
        self.flat(tile.map_or([0; 3], |tile| terrain_shift(tile.time_color)));
    }
}

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
uniform mat4 Model;
uniform mat4 Projection;
varying mediump vec2 uv;
varying mediump vec2 world;
varying lowp vec4 color;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    world = position.xy;
    uv = texcoord;
    color = color0 / 255.0;
}"#;

const FRAGMENT: &str = r#"#version 100
precision mediump float;
uniform sampler2D Texture;
uniform sampler2D Masks;
uniform vec3 Shift;
uniform vec3 HexBox;
uniform vec3 Adjacent[7];
varying mediump vec2 uv;
varying mediump vec2 world;
varying lowp vec4 color;
vec4 mask_sample(float direction, float row, vec2 p) {
    vec2 local = clamp(p * 72.0, vec2(0.5), vec2(71.5));
    return texture2D(Masks, (vec2(direction, row) * 72.0 + local) / vec2(432.0, 216.0));
}
void main() {
    vec3 adjustment = Shift;
    if (HexBox.z > 0.0) {
        vec2 p = (world - HexBox.xy) / HexBox.z;
        for (int group = 0; group < 3; ++group) {
            // Native order: concave, first convex edge, second convex edge.
            for (int d = 0; d < 6; ++d) {
                vec3 a = Adjacent[d];
                vec3 b = Adjacent[d + 1];
                bool concave = all(equal(a,b)) && any(notEqual(a,Shift));
                bool first = any(notEqual(a,b)) && any(notEqual(a,Shift));
                bool second = any(notEqual(a,b)) && any(notEqual(b,Shift));
                if ((group == 0 && concave) || (group == 1 && first) || (group == 2 && second)) {
                    vec4 mask = mask_sample(float(d), float(group), p);
                    vec3 lit = clamp(mask.rgb + (group == 2 ? b : a) * 0.5, 0.0, 1.0);
                    adjustment = mix(adjustment, (lit - 128.0 / 255.0) * 2.0, mask.a);
                }
            }
        }
    }
    vec4 pixel = texture2D(Texture, uv);
    gl_FragColor = vec4(clamp(pixel.rgb + adjustment, 0.0, 1.0), pixel.a) * color;
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_shifts_match_the_native_signed_half_channel_encoding() {
        assert_eq!(terrain_shift([-25, -15, 0]), [-24, -14, 0]);
        assert_eq!(terrain_shift([10, -20, -35]), [10, -20, -34]);
        assert_eq!(terrain_shift([-999, 999, -1]), [-256, 254, 0]);
    }

    #[test]
    fn uniform_light_stores_one_shift_and_regional_light_keeps_neighbor_masks() {
        let north = Hex { x: 1, y: 1 };
        let south = Hex { x: 1, y: 2 };
        let tile = |position| ObservedTile {
            position, terrain: "Gg".into(), visible: true,
            village_owner: None, time_color: [-25, -15, 0],
        };
        let mut view = Observation {
            side: 1, allied_sides: vec![1], gold: None, width: 3, height: 3,
            tiles: vec![tile(north), tile(south)], units: vec![],
        };
        let mut terrain = TerrainLighting::default();
        terrain.update(&view);
        let night = [-24, -14, 0];
        assert!(matches!(terrain, TerrainLighting::Uniform(shift) if shift == night));
        assert_eq!(terrain.shade(north), (night, None));

        view.tiles[1].time_color = [26, 10, 0];
        terrain.update(&view);
        let (shift, neighbors) = terrain.shade(north);
        assert_eq!(shift, night);
        let expected = Direction::CLOCKWISE.map(|dir| {
            if dir == Direction::South { [26, 10, 0] } else { night }
        });
        assert_eq!(neighbors, Some(&expected));

        view.tiles.pop();
        terrain.update(&view);
        assert!(matches!(terrain, TerrainLighting::Uniform(shift) if shift == night));
    }
}

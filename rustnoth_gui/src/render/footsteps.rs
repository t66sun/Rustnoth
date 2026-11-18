//! Native footstep halves, teleport flashes and the single-turn destination mark.
use super::{BoardLayout, load_texture};
use engine_lite::{
    fixed_match::move_preview::MovePreview,
    hex::{Direction, Hex},
};
use macroquad::prelude::*;
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Print {
    Foot {
        direction: usize,
        reversed: bool,
        outgoing: bool,
    },
    Teleport {
        outgoing: bool,
    },
}

fn half(from: Hex, to: Hex, outgoing: bool) -> Print {
    match Direction::CLOCKWISE
        .iter()
        .position(|&dir| from.neighbor(dir) == Some(to))
    {
        Some(n) => Print::Foot {
            direction: n % 3,
            reversed: n >= 3,
            outgoing,
        },
        None => Print::Teleport { outgoing },
    }
}

fn halves(steps: &[(Hex, u32)], i: usize) -> [Option<Print>; 2] {
    let at = steps[i].0;
    let incoming = i
        .checked_sub(1)
        .map(|previous| half(steps[previous].0, at, false));
    let outgoing = steps.get(i + 1).map(|&(next, _)| half(at, next, true));
    // The teleport flash is drawn above any adjoining walking half.
    match (incoming, outgoing) {
        (Some(Print::Teleport { .. }), Some(Print::Foot { .. })) => [outgoing, incoming],
        (Some(Print::Teleport { .. }), Some(Print::Teleport { .. })) => [None, outgoing],
        _ => [incoming, outgoing],
    }
}

pub(super) struct Art {
    speeds: Vec<[[Option<Texture2D>; 3]; 2]>,
    teleports: [Option<Texture2D>; 2],
    marks: [Option<Texture2D>; 3],
    colors: Vec<Color>,
}

impl Art {
    pub(super) fn load(root: &Path) -> Self {
        let folder = root.join("images");
        let config = fs::read_to_string(root.join("data/game_config.cfg")).unwrap_or_default();
        let value = |name: &str| {
            config
                .lines()
                .filter_map(|line| line.split_once('='))
                .find(|(key, _)| key.trim() == name)
                .map(|(_, val)| val.trim().trim_matches('"'))
        };
        let speeds = value("footprint_prefix")
            .unwrap_or("footsteps/foot-normal,footsteps/foot-medium,footsteps/foot-slow")
            .split(',')
            .map(|prefix| {
                ["in", "out"].map(|sense| {
                    ["n", "ne", "se"].map(|dir| {
                        load_texture(
                            &folder.join(format!("{}-{sense}-{dir}.png", prefix.trim())),
                            None,
                        )
                    })
                })
            })
            .collect();
        let teleports = [
            value("footprint_teleport_enter").unwrap_or("footsteps/teleport-in.png"),
            value("footprint_teleport_exit").unwrap_or("footsteps/teleport-out.png"),
        ]
        .map(|file| load_texture(&folder.join(file), None));
        let marks = ["misc/hidden.png", "misc/zoc.png", "misc/capture.png"]
            .map(|file| load_texture(&folder.join(file), None));
        let colors = value("red_green_scale")
            .unwrap_or("")
            .split(',')
            .filter_map(|c| u32::from_str_radix(c.trim(), 16).ok())
            .map(|rgb| Color::from_rgba((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255))
            .collect();
        Self {
            speeds,
            teleports,
            marks,
            colors,
        }
    }

    pub(super) fn draw_steps(&self, layout: &BoardLayout, route: &MovePreview) {
        for (index, &(at, cost)) in route.steps.iter().enumerate() {
            let speed = (cost as usize).min(self.speeds.len()).checked_sub(1);
            let [x, y] = layout.tile_origin(at);
            for print in halves(&route.steps, index).into_iter().flatten() {
                let (texture, reverse) = match print {
                    Print::Foot {
                        direction,
                        reversed,
                        outgoing,
                    } => (
                        speed.and_then(|n| {
                            self.speeds[n][usize::from(outgoing)][direction].as_ref()
                        }),
                        reversed,
                    ),
                    Print::Teleport { outgoing } => {
                        (self.teleports[usize::from(outgoing)].as_ref(), false)
                    }
                };
                if let Some(texture) = texture {
                    draw_texture_ex(
                        texture,
                        x,
                        y,
                        WHITE,
                        DrawTextureParams {
                            dest_size: Some(Vec2::splat(layout.tile_size())),
                            flip_x: reverse,
                            flip_y: reverse,
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }

    pub(super) fn draw_mark(&self, layout: &BoardLayout, route: &MovePreview, font: Option<&Font>) {
        let Some(&(dest, _)) = route.steps.last() else {
            return;
        };
        let size = layout.tile_size();
        let [x, y] = layout.tile_origin(dest);
        let label = format!("{}%", route.defense);
        let pixels = (18.0 * size / 72.0).round().max(1.0) as u16;
        let text = measure_text(&label, font, pixels, 1.0);
        let left = x + (size - text.width) / 2.0;
        let baseline = y + size / 2.0 + text.offset_y - text.height / 2.0;
        let level = (f32::from(route.defense) * 0.01 * self.colors.len().saturating_sub(1) as f32)
            .round_ties_even() as usize;
        let color = self.colors.get(level).copied().unwrap_or(WHITE);
        for (dx, dy, tint) in [
            (-1.0, 0.0, BLACK),
            (1.0, 0.0, BLACK),
            (0.0, -1.0, BLACK),
            (0.0, 1.0, BLACK),
            (0.0, 0.0, color),
        ] {
            draw_text_ex(
                &label,
                left + dx,
                baseline + dy,
                TextParams {
                    font,
                    font_size: pixels,
                    color: tint,
                    ..Default::default()
                },
            );
        }
        for (enabled, mark) in [route.hidden, route.zone_of_control, route.capture]
            .into_iter()
            .zip(&self.marks)
        {
            if enabled && let Some(image) = mark {
                draw_texture_ex(
                    image,
                    x,
                    y,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(Vec2::splat(size)),
                        ..Default::default()
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_join_native_halves_and_teleport_flashes_go_last() {
        let route = [
            (Hex { x: 0, y: 0 }, 1),
            (Hex { x: 1, y: 0 }, 2),
            (Hex { x: 1, y: 1 }, 1),
            (Hex { x: 6, y: 5 }, 1),
            (Hex { x: 5, y: 4 }, 1),
        ];
        let foot = |direction, reversed, outgoing| {
            Some(Print::Foot {
                direction,
                reversed,
                outgoing,
            })
        };
        assert_eq!(halves(&route, 0), [None, foot(2, false, true)]);
        assert_eq!(
            halves(&route, 1),
            [foot(2, false, false), foot(0, true, true)]
        );
        assert_eq!(
            halves(&route, 2),
            [
                foot(0, true, false),
                Some(Print::Teleport { outgoing: true })
            ]
        );
        assert_eq!(
            halves(&route, 3),
            [
                foot(2, true, true),
                Some(Print::Teleport { outgoing: false })
            ]
        );
        assert_eq!(halves(&route, 4), [foot(2, true, false), None]);
    }
}

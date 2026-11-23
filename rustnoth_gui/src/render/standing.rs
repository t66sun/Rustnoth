//! Primary unit frames from the stock [standing_anim] definitions.
use super::{atlas::AtlasSprite, rules::pattern};
use engine_lite::{hex::Direction, observation::ObservedUnit, wml::Node};
use std::collections::HashMap;

pub(super) fn facing(unit: &ObservedUnit) -> Direction {
    // Native picks a random initial facing. Keep this cosmetic choice stable without
    // advancing the rules RNG or relying on the number of hidden units created.
    unit.facing.unwrap_or_else(|| {
        let x = unit.position.x as u32;
        let y = unit.position.y as u32;
        let seed = x.wrapping_mul(7349) ^ y.wrapping_mul(9151) ^ unit.side as u32;
        Direction::CLOCKWISE[seed as usize % 6]
    })
}

pub(super) fn west(direction: Direction) -> bool {
    matches!(direction, Direction::SouthWest | Direction::NorthWest)
}

pub(super) struct Frame {
    pub(super) path: String,
    pub(super) end_ms: u64,
    pub(super) auto_hflip: bool,
}

struct Cycle {
    frames: Vec<Frame>,
    terrain: Option<String>,
    specific_direction: bool,
}

impl Cycle {
    fn frame(&self, now: u64) -> &Frame {
        let position = now % self.frames.last().unwrap().end_ms;
        &self.frames[self.frames.partition_point(|f| f.end_ms <= position)]
    }
}

pub(super) struct Sprites {
    directions: [Vec<Cycle>; 6],
    textures: HashMap<String, Option<AtlasSprite>>,
}

impl Sprites {
    pub(super) fn load(
        definitions: &[Node],
        mut load: impl FnMut(&str) -> Option<AtlasSprite>,
    ) -> Self {
        let mut textures = HashMap::new();
        let directions = Direction::CLOCKWISE.map(|direction| {
            let cycles: Vec<_> = definitions
                .iter()
                .filter_map(|n| cycle(n, direction))
                .collect();
            for frame in cycles.iter().flat_map(|c| &c.frames) {
                textures
                    .entry(frame.path.clone())
                    .or_insert_with(|| load(&frame.path));
            }
            cycles
        });
        Self {
            directions,
            textures,
        }
    }

    pub(super) fn frame(
        &self,
        direction: Direction,
        terrain: Option<&str>,
        now: u64,
    ) -> Option<(&AtlasSprite, bool)> {
        let cycles = &self.directions[direction as usize];
        let cycle = cycles
            .iter()
            .filter(|c| {
                c.terrain
                    .as_ref()
                    .is_none_or(|filter| terrain.is_some_and(|code| pattern::matches(filter, code)))
            })
            .max_by_key(|c| usize::from(c.specific_direction) + usize::from(c.terrain.is_some()))?;
        let frame = cycle.frame(now);
        Some((
            self.textures.get(&frame.path)?.as_ref()?,
            west(direction) && frame.auto_hflip,
        ))
    }
}

fn direction_matches(node: &Node, facing: Direction) -> bool {
    node.attribute("direction").is_none_or(|directions| {
        directions
            .split(',')
            .filter_map(Direction::from_wml)
            .any(|dir| dir == facing)
    })
}

// Filtered and multi-particle animations require their own WML semantics. Only
// use a definition when the main sprite and its conditions can be represented.
fn supported(node: &Node) -> bool {
    node.attributes.keys().all(|key| {
        matches!(
            key.as_str(),
            "direction" | "terrain_type" | "start_time" | "cycles" | "auto_hflip" | "layer"
        )
    }) && node
        .children
        .iter()
        .all(|(tag, _)| matches!(tag.as_str(), "frame" | "if" | "else"))
}

fn cycle(anim: &Node, direction: Direction) -> Option<Cycle> {
    if !supported(anim) || !direction_matches(anim, direction) {
        return None;
    }
    let mut frames = Vec::new();
    let mut specific_direction = anim.attribute("direction").is_some();
    let mut matched = false;
    let inherited_flip = hflip(anim, true);
    for (tag, child) in &anim.children {
        if tag == "frame" {
            append_frame(&mut frames, child, inherited_flip)?;
            continue;
        }
        if !supported(child)
            || child.attribute("terrain_type").is_some()
            || child.children.iter().any(|(name, _)| name != "frame")
        {
            return None;
        }
        if tag == "if" {
            matched = false;
        }
        if matched || !direction_matches(child, direction) {
            continue;
        }
        matched = true;
        specific_direction |= child.attribute("direction").is_some();
        for (_, frame) in &child.children {
            append_frame(&mut frames, frame, hflip(child, inherited_flip))?;
        }
    }
    (!frames.is_empty()).then(|| Cycle {
        frames,
        terrain: anim.attribute("terrain_type").map(str::to_owned),
        specific_direction,
    })
}

fn hflip(node: &Node, default: bool) -> bool {
    node.attribute("auto_hflip")
        .map_or(default, |s| matches!(s.trim(), "yes" | "true" | "on" | "1"))
}

fn append_frame(frames: &mut Vec<Frame>, frame: &Node, parent_flip: bool) -> Option<()> {
    append_sprite_frame(frames, frame, parent_flip, false)
}

pub(super) fn append_sprite_frame(frames: &mut Vec<Frame>, frame: &Node, parent_flip: bool, allow_blit: bool) -> Option<()> {
    let images = pattern::expand(frame.attribute("image")?);
    if images.is_empty() {
        return None;
    }
    let duration = if let Some(ms) = frame.attribute("duration") {
        Some(ms.parse::<u64>().ok()?)
    } else if let Some(end) = frame.attribute("end") {
        let begin = frame
            .attribute("begin")
            .unwrap_or("0")
            .parse::<i64>()
            .ok()?;
        Some(u64::try_from(end.parse::<i64>().ok()?.checked_sub(begin)?).ok()?)
    } else {
        None
    };
    let mut specified_ms = 0u64;
    let mut parsed = Vec::new();
    for image in &images {
        let (path, time) = match image.rsplit_once(':') {
            Some((path, ms)) => (path, Some(ms.parse::<u64>().ok()?)),
            None => (image.as_str(), None),
        };
        let base = path.split('~').next()?;
        if !base.ends_with(".png") || path.contains('$') || path.contains('~') && !allow_blit {
            return None;
        }
        specified_ms = specified_ms.checked_add(time.unwrap_or(0))?;
        parsed.push((path, time));
    }

    // progressive_single divides the unspecified remainder by the number of
    // expanded entries. The enclosing [frame] can clip or hold its final image.
    let chunk = duration.unwrap_or(1).saturating_sub(specified_ms) / parsed.len() as u64;
    let start = frames.last().map_or(0, |f| f.end_ms);
    let limit = match duration {
        Some(ms) => start.checked_add(ms)?,
        None => u64::MAX,
    };
    let mut end = start;
    for (path, time) in parsed {
        let next = end.checked_add(time.unwrap_or(chunk.max(1)))?.min(limit);
        if next > end {
            frames.push(Frame {
                path: path.to_owned(),
                end_ms: next,
                auto_hflip: hflip(frame, parent_flip),
            });
            end = next;
        }
        if end == limit {
            break;
        }
    }
    if duration.is_some() && end > start {
        frames.last_mut()?.end_ms = limit;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::wml;

    #[test]
    fn stock_bat_durations_wrap_and_western_facings_mirror() {
        let anim = wml::parse(
            r#"direction=n,ne,nw
[frame]
image="units/bats/bat-ne-[3~1,2~5,4].png:[50,60,80,60,50,60,80,60]"
[/frame]"#,
        )
        .unwrap();
        let cycle = cycle(&anim, Direction::NorthWest).unwrap();
        let frames: Vec<_> = [0, 49, 50, 109, 110, 439, 440, 500]
            .map(|t| cycle.frame(t).path.strip_prefix("units/bats/").unwrap())
            .into_iter()
            .collect();
        assert_eq!(
            frames,
            [
                "bat-ne-3.png",
                "bat-ne-3.png",
                "bat-ne-2.png",
                "bat-ne-2.png",
                "bat-ne-1.png",
                "bat-ne-5.png",
                "bat-ne-4.png",
                "bat-ne-3.png"
            ]
        );
        assert!(west(Direction::NorthWest));
        assert!(!west(Direction::North));
    }

    #[test]
    fn spearman_uses_its_directional_branch_and_skips_wounded_filters() {
        let anim = wml::parse(
            r#"[if]
direction=s,se,sw
[frame]
image="spearman-stand-s-[1~7,6,7~2].png:200"
[/frame]
[/if]
[else]
direction=n,ne,nw
[frame]
image="spearman-stand-n-[1~5,4,5,4,5~2].png:200"
[/frame]
[/else]"#,
        )
        .unwrap();
        assert_eq!(
            cycle(&anim, Direction::SouthWest).unwrap().frame(200).path,
            "spearman-stand-s-2.png"
        );
        assert_eq!(
            cycle(&anim, Direction::NorthEast).unwrap().frame(1000).path,
            "spearman-stand-n-4.png"
        );
        let mut wounded = anim;
        wounded.children.push(("filter".into(), Node::default()));
        assert!(cycle(&wounded, Direction::SouthEast).is_none());
    }

    #[test]
    fn explicit_durations_hold_the_final_image_and_respect_frame_hflip() {
        let anim = wml::parse(
            r#"auto_hflip=no
[frame]
duration=100
image="first.png:20,last.png:30"
[/frame]
[frame]
auto_hflip=yes
image="west.png:50"
[/frame]"#,
        )
        .unwrap();
        let cycle = cycle(&anim, Direction::SouthWest).unwrap();
        assert_eq!(cycle.frame(99).path, "last.png");
        assert!(!cycle.frame(99).auto_hflip);
        assert!(cycle.frame(100).auto_hflip);
        assert_eq!(cycle.frame(150).path, "first.png");
    }
}

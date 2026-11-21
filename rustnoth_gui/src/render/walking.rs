//! Player-facing movement after the referee has resolved the walk.
use super::{BoardLayout, Hex, Vec2};
use engine_lite::hex::Direction;

const STEP_MS: u64 = 200;

#[derive(Default)]
pub(crate) struct Walk {
    route: Vec<Hex>,
    started: u64,
    rate: f32,
}

#[derive(Clone, Copy)]
pub(super) struct Pose {
    pub unit: Hex,
    from: Hex,
    pub to: Hex,
    pub direction: Option<Direction>,
    fraction: f32,
    pub elapsed: u64,
}

impl Walk {
    pub(crate) fn start(&mut self, route: Vec<Hex>, now: u64, rate: f32) {
        self.route = route;
        self.started = now;
        self.rate = rate;
    }

    pub(crate) fn active(&self) -> bool {
        self.route.len() >= 2
    }

    /// A skip is consumed by this animation. It cannot also end the turn or quit the map.
    pub(crate) fn advance(&mut self, now: u64, skip: bool) -> bool {
        let was_active = self.active();
        if skip || self.pose(now).is_none() {
            self.route.clear();
        }
        was_active
    }

    pub(crate) fn next_hex(&self, now: u64) -> Option<Hex> {
        self.pose(now).map(|p|p.to)
    }

    pub(super) fn pose(&self, now: u64) -> Option<Pose> {
        let elapsed = (now.saturating_sub(self.started) as f64 * f64::from(self.rate)) as u64;
        let index = usize::try_from(elapsed / STEP_MS).ok()?;
        let from = *self.route.get(index)?;
        let to = *self.route.get(index.checked_add(1)?)?;
        let direction = from.direction_to_neighbor(to);
        let progress = (elapsed % STEP_MS) as f32 / STEP_MS as f32;
        // A tunnel moves directly between its endpoints, never through the intervening map.
        let fraction = if direction.is_some() { progress } else if progress < 0.5 { 0.0 } else { 1.0 };
        Some(Pose { unit: *self.route.last()?, from, to, direction, fraction, elapsed })
    }
}

impl Pose {
    pub(super) fn origin(&self, layout: &BoardLayout) -> Vec2 {
        Vec2::from_array(layout.tile_origin(self.from))
            .lerp(Vec2::from_array(layout.tile_origin(self.to)), self.fraction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movement_reaches_each_actual_step_and_teleports_without_crossing_the_map() {
        let a = Hex { x: 0, y: 0 };
        let b = Hex { x: 1, y: 0 };
        let c = Hex { x: 9, y: 4 };
        let mut walk = Walk::default();
        walk.start(vec![a, b, c], 40, 1.0);
        let first = walk.pose(140).unwrap();
        assert_eq!((first.from, first.to, first.fraction, first.direction), (a, b, 0.5, Some(Direction::SouthEast)));
        assert_eq!(walk.pose(330).unwrap().fraction, 0.0);
        assert_eq!(walk.pose(340).unwrap().fraction, 1.0);
        assert!(walk.pose(440).is_none());
        assert!(walk.advance(440, false));
        assert!(!walk.active());
        walk.start(vec![a, b], 1000, 2.0);
        assert_eq!(walk.pose(1050).unwrap().fraction, 0.5);
        assert!(walk.advance(1033, true));
        assert!(!walk.advance(1034, false));
    }
}

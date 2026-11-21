//! Player-facing movement after the referee has resolved the walk.
use super::{BoardLayout, Hex, Vec2};
use engine_lite::hex::Direction;

const STEP_MS: u64 = 200;

#[derive(Default)]
pub(crate) struct Walk {
    steps: Vec<(Hex, Hex)>,
    unit: Option<Hex>,
    pub(super) transient_sprite: Option<(String, usize)>,
    started: u64,
    rate: f32,
}

#[derive(Clone, Copy)]
pub(super) struct Pose {
    pub unit: Option<Hex>,
    from: Hex,
    pub to: Hex,
    pub direction: Option<Direction>,
    fraction: f32,
    pub elapsed: u64,
}

impl Walk {
    pub(crate) fn start(&mut self, route: Vec<Hex>, now: u64, rate: f32) {
        self.steps = route.windows(2).map(|p| (p[0], p[1])).collect();
        self.unit = route.last().copied();
        self.transient_sprite = None;
        self.started = now;
        self.rate = rate;
    }

    pub(super) fn start_observed(&mut self, walk: engine_lite::fixed_match::walk::VisibleWalk, now: u64, rate: f32) {
        self.steps = walk.edges;
        self.unit = walk.visible_destination;
        self.transient_sprite = self.unit.is_none().then_some((walk.unit_type, walk.side));
        self.started = now;
        self.rate = rate;
    }

    pub(crate) fn active(&self) -> bool {
        !self.steps.is_empty()
    }

    /// A skip is consumed by this animation. It cannot also end the turn or quit the map.
    pub(crate) fn advance(&mut self, now: u64, skip: bool) -> bool {
        let was_active = self.active();
        if skip || self.pose(now).is_none() {
            self.steps.clear();
        }
        was_active
    }

    pub(crate) fn next_hex(&self, now: u64) -> Option<Hex> {
        self.pose(now).map(|p|p.to)
    }

    pub(super) fn pose(&self, now: u64) -> Option<Pose> {
        let elapsed = (now.saturating_sub(self.started) as f64 * f64::from(self.rate)) as u64;
        let index = usize::try_from(elapsed / STEP_MS).ok()?;
        let &(from, to) = self.steps.get(index)?;
        let direction = from.direction_to_neighbor(to);
        let progress = (elapsed % STEP_MS) as f32 / STEP_MS as f32;
        // A tunnel moves directly between its endpoints, never through the intervening map.
        let fraction = if direction.is_some() { progress } else if progress < 0.5 { 0.0 } else { 1.0 };
        Some(Pose { unit: self.unit, from, to, direction, fraction, elapsed })
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
    fn an_observer_never_interpolates_across_unseen_steps() {
        let at = |x| Hex { x, y: 0 };
        let mut walk = Walk::default();
        let visible = engine_lite::fixed_match::walk::VisibleWalk {
            unit_type: "Scout".into(), side: 2, visible_destination: None,
            edges: vec![(at(0), at(1)), (at(4), at(5))],
        };
        walk.start_observed(visible, 0, 1.);
        let before = walk.pose(199).unwrap();
        assert_eq!((before.from, before.to, before.unit), (at(0), at(1), None));
        let after = walk.pose(200).unwrap();
        assert_eq!((after.from, after.to), (at(4), at(5)));
        assert!(walk.pose(400).is_none());
    }

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

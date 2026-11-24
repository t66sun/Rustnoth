//! Player-facing movement after the referee has resolved the walk.
use super::Hex;
use engine_lite::hex::Direction;

const STEP_MS: u64 = 200;

#[derive(Default)]
pub(crate) struct Walk {
    steps: Vec<(Hex, Hex)>,
    unit: Option<Hex>,
    pub(super) transient_sprite: Option<(String, usize)>,
    started: u64,
    rate: f32,
    camera_taken: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Pose {
    pub unit: Option<Hex>,
    pub from: Hex,
    pub to: Hex,
    pub direction: Option<Direction>,
    pub fraction: f32,
    pub elapsed: u64,
}

impl Walk {
    pub(crate) fn start(&mut self, route: Vec<Hex>, now: u64, rate: f32) {
        self.steps = route.windows(2).map(|p| (p[0], p[1])).collect();
        self.unit = route.last().copied();
        self.transient_sprite = None;
        self.started = now;
        self.rate = rate;
        self.camera_taken = false;
    }

    pub(super) fn start_observed(&mut self, walk: engine_lite::fixed_match::walk::VisibleWalk, now: u64, rate: f32) {
        self.steps = walk.edges;
        self.unit = walk.visible_destination;
        self.transient_sprite = self.unit.is_none().then_some((walk.unit_type, walk.side));
        self.started = now;
        self.rate = rate;
        self.camera_taken = false;
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

    pub(crate) fn camera_target(&mut self, now: u64, drag_takes_camera: bool) -> Option<Hex> {
        self.camera_taken |= drag_takes_camera;
        if self.camera_taken {
            return None;
        }
        self.pose(now).map(|pose| pose.to)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_middle_drag_takes_camera_follow_until_the_next_walk() {
        let a = Hex { x: 0, y: 0 };
        let b = Hex { x: 1, y: 0 };
        let c = Hex { x: 2, y: 0 };
        let mut walk = Walk::default();

        walk.start(vec![a, b, c], 0, 1.0);
        assert_eq!(walk.camera_target(10, false), Some(b));
        assert_eq!(walk.camera_target(30, true), None);
        assert_eq!(walk.camera_target(250, false), None);
        assert_eq!(walk.pose(250).unwrap().to, c);

        walk.start_observed(engine_lite::fixed_match::walk::VisibleWalk {
            unit_type: "Scout".into(), side: 2, visible_destination: Some(c), edges: vec![(b, c)],
        }, 400, 1.0);
        assert_eq!(walk.camera_target(410, false), Some(c));
        assert_eq!(walk.camera_target(430, true), None);

        walk.start(vec![b, a], 600, 1.0);
        assert_eq!(walk.camera_target(610, false), Some(a));
    }

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

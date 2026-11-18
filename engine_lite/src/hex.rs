/// A hex in Wesnoth's zero-based map coordinates.
///
/// Odd columns are shifted half a hex south. WML uses one-based coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Hex {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    North,
    NorthEast,
    SouthEast,
    South,
    SouthWest,
    NorthWest,
}

impl Direction {
    pub fn from_wml(name: &str) -> Option<Self> {
        match name.trim() {
            "n" => Some(Self::North),
            "ne" => Some(Self::NorthEast),
            "se" => Some(Self::SouthEast),
            "s" => Some(Self::South),
            "sw" => Some(Self::SouthWest),
            "nw" => Some(Self::NorthWest),
            _ => None,
        }
    }

    pub const CLOCKWISE: [Self; 6] = [
        Self::North,
        Self::NorthEast,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::NorthWest,
    ];
}

impl Hex {
    pub const fn from_wml(x: i32, y: i32) -> Option<Self> {
        match (x.checked_sub(1), y.checked_sub(1)) {
            (Some(x), Some(y)) => Some(Self { x, y }),
            _ => None,
        }
    }

    pub const fn to_wml(self) -> Option<(i32, i32)> {
        match (self.x.checked_add(1), self.y.checked_add(1)) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        }
    }

    pub fn neighbor(self, direction: Direction) -> Option<Self> {
        let shift = self.x.rem_euclid(2);
        let (dx, dy) = match direction {
            Direction::North => (0, -1),
            Direction::NorthEast => (1, shift - 1),
            Direction::SouthEast => (1, shift),
            Direction::South => (0, 1),
            Direction::SouthWest => (-1, shift),
            Direction::NorthWest => (-1, shift - 1),
        };
        Some(Self {
            x: self.x.checked_add(dx)?,
            y: self.y.checked_add(dy)?,
        })
    }

    /// Native radial direction. Ties rotate clockwise at each sector boundary.
    pub fn direction_to(self, other: Self) -> Option<Direction> {
        let dx = i64::from(other.x) - i64::from(self.x);
        let dy = 2 * (i64::from(other.y) - i64::from(self.y))
            + i64::from(other.x.rem_euclid(2) - self.x.rem_euclid(2));
        // Vertical displacements after rotating the hex vector clockwise by 60 and 120 degrees.
        let after_one = 3 * dx + dy;
        let after_two = 3 * dx - dy;
        Some(match dy.cmp(&0) {
            std::cmp::Ordering::Less if after_one >= 0 => Direction::NorthEast,
            std::cmp::Ordering::Less if after_two >= 0 => Direction::North,
            std::cmp::Ordering::Less => Direction::NorthWest,
            std::cmp::Ordering::Greater if after_one <= 0 => Direction::SouthWest,
            std::cmp::Ordering::Greater if after_two <= 0 => Direction::South,
            std::cmp::Ordering::Greater => Direction::SouthEast,
            std::cmp::Ordering::Equal if dx > 0 => Direction::SouthEast,
            std::cmp::Ordering::Equal if dx < 0 => Direction::NorthWest,
            _ => return None,
        })
    }

    pub fn direction_to_neighbor(self, other: Self) -> Option<Direction> {
        Direction::CLOCKWISE.into_iter().find(|&dir| self.neighbor(dir) == Some(other))
    }

    pub fn neighbors(self) -> impl Iterator<Item = Self> {
        Direction::CLOCKWISE
            .into_iter()
            .filter_map(move |direction| self.neighbor(direction))
    }

    pub fn distance(self, other: Self) -> u64 {
        // Remove the column stagger to get axial coordinates.
        let q = i64::from(self.x);
        let r = i64::from(self.y) - q.div_euclid(2);
        let other_q = i64::from(other.x);
        let other_r = i64::from(other.y) - other_q.div_euclid(2);
        let dq = q - other_q;
        let dr = r - other_r;
        dq.unsigned_abs().max(dr.unsigned_abs()).max((dq + dr).unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wml_coordinates_include_the_border() {
        assert_eq!(Hex::from_wml(1, 1), Some(Hex { x: 0, y: 0 }));
        assert_eq!(Hex::from_wml(0, 3), Some(Hex { x: -1, y: 2 }));
        assert_eq!(Hex { x: -1, y: 2 }.to_wml(), Some((0, 3)));
        assert_eq!(Hex::from_wml(i32::MIN, 1), None);
    }

    #[test]
    fn neighbors_follow_column_parity_in_clockwise_order() {
        assert_eq!(
            Hex { x: 0, y: 0 }.neighbors().collect::<Vec<_>>(),
            [
                Hex { x: 0, y: -1 },
                Hex { x: 1, y: -1 },
                Hex { x: 1, y: 0 },
                Hex { x: 0, y: 1 },
                Hex { x: -1, y: 0 },
                Hex { x: -1, y: -1 },
            ]
        );
        assert_eq!(
            Hex { x: -1, y: 0 }.neighbors().collect::<Vec<_>>(),
            [
                Hex { x: -1, y: -1 },
                Hex { x: 0, y: 0 },
                Hex { x: 0, y: 1 },
                Hex { x: -1, y: 1 },
                Hex { x: -2, y: 1 },
                Hex { x: -2, y: 0 },
            ]
        );
    }

    #[test]
    fn distance_counts_hex_steps() {
        let origin = Hex { x: 0, y: 0 };
        assert_eq!(origin.distance(origin), 0);
        for neighbor in origin.neighbors() {
            assert_eq!(origin.distance(neighbor), 1);
            assert_eq!(neighbor.distance(origin), 1);
        }
        assert_eq!(origin.distance(Hex { x: 4, y: -3 }), 5);
        assert_eq!(origin.distance(Hex { x: -4, y: 2 }), 4);
    }
}

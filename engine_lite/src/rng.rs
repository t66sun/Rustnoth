//! The deterministic random stream used in referee state and replay.

const STATE_SIZE: usize = 624;
const SHIFT: usize = 397;
const UPPER_BIT: u32 = 0x8000_0000;
const LOWER_BITS: u32 = UPPER_BIT - 1;

/// Wesnoth's seeded 32-bit MT19937 stream.
///
/// A replay checkpoint records the seed and number of draws already consumed.
/// Keep the stream and checkpoint inside the referee and replay paths.
#[derive(Clone)]
pub struct ReplayRng {
    state: [u32; STATE_SIZE],
    cursor: usize,
    seed: u32,
    calls: u32,
}

impl ReplayRng {
    pub fn new(seed: u32) -> Self {
        let mut state = [0u32; STATE_SIZE];
        state[0] = seed;
        for position in 1..STATE_SIZE {
            let previous = state[position - 1];
            state[position] = 1_812_433_253u32
                .wrapping_mul(previous ^ (previous >> 30))
                .wrapping_add(position as u32);
        }
        Self {
            state,
            cursor: STATE_SIZE,
            seed,
            calls: 0,
        }
    }

    pub fn restore(seed: u32, calls: u32) -> Self {
        let mut rng = Self::new(seed);
        for _ in 0..calls {
            rng.next_u32();
        }
        rng
    }

    pub fn seed_and_calls(&self) -> (u32, u32) {
        (self.seed, self.calls)
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.cursor == STATE_SIZE {
            self.twist();
            self.cursor = 0;
        }

        let mut word = self.state[self.cursor];
        self.cursor += 1;
        self.calls = self.calls.wrapping_add(1);
        word ^= word >> 11;
        word ^= (word << 7) & 0x9d2c_5680;
        word ^= (word << 15) & 0xefc6_0000;
        word ^ (word >> 18)
    }

    fn twist(&mut self) {
        // Indices that wrap around refer to the words already updated this pass.
        for position in 0..STATE_SIZE {
            let next = self.state[(position + 1) % STATE_SIZE];
            let joined = (self.state[position] & UPPER_BIT) | (next & LOWER_BITS);
            let correction = if joined & 1 == 0 { 0 } else { 0x9908_b0df };
            self.state[position] =
                self.state[(position + SHIFT) % STATE_SIZE] ^ (joined >> 1) ^ correction;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReplayRng;

    #[test]
    fn matches_cpp_mt19937_reference_outputs() {
        let mut rng = ReplayRng::new(5489);
        assert_eq!(rng.next_u32(), 3_499_211_612);
        assert_eq!(rng.next_u32(), 581_869_302);
        // The C++ standard specifies this 10,000th output.
        assert_eq!(ReplayRng::restore(5489, 9999).next_u32(), 4_123_659_995);
    }

    #[test]
    fn restores_the_next_draw_on_either_side_of_a_twist() {
        let seed = 0x5eed_f00d;
        let mut uninterrupted = ReplayRng::new(seed);
        for _ in 0..623 {
            uninterrupted.next_u32();
        }
        for expected_count in 623..=625 {
            let (seed, calls) = uninterrupted.seed_and_calls();
            assert_eq!(calls, expected_count);
            let mut restored = ReplayRng::restore(seed, calls);
            assert_eq!(restored.next_u32(), uninterrupted.next_u32());
            assert_eq!(restored.seed_and_calls(), (seed, expected_count + 1));
        }
    }
}

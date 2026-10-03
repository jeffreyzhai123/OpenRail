//! Seeded PRNG: Tommy Ettinger's mulberry32. Hand-rolled (no `rand`) so every
//! draw is a pure function of the seed, which replay-by-seed depends on.
//!
//! The exact algorithms here, including how `below` and `shuffle` consume
//! draws, are part of the replay contract: a seed's fault plan is built from
//! them, so changing one changes every seed's run. The golden tests pin them.

use std::num::NonZeroU32;
use std::ops::Range;

/// mulberry32's Weyl-sequence step.
const MULBERRY32_INCREMENT: u32 = 0x6D2B_79F5;

/// The `u32` seed is the whole state: one seed, one stream.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub fn from_seed(seed: u32) -> Rng {
        Rng { state: seed }
    }

    /// mulberry32 is defined mod 2^32, so every step wraps.
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(MULBERRY32_INCREMENT);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    /// Uniform in `range`, or `None` if it's empty. An empty range draws
    /// nothing, so it doesn't shift later draws.
    pub fn next_range(&mut self, range: Range<u32>) -> Option<u32> {
        let width = NonZeroU32::new(range.end.checked_sub(range.start)?)?;
        Some(range.start + self.below(width))
    }

    /// Fisher–Yates from the back.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        assert!(
            items.len() <= u32::MAX as usize,
            "shuffle supports at most u32::MAX items"
        );
        for i in (1..items.len()).rev() {
            // `i + 1` without an `Option`: it can't be zero.
            let bound = NonZeroU32::MIN.saturating_add(i as u32);
            let j = self.below(bound);
            items.swap(i, j as usize);
        }
    }

    /// Uniform in `0..bound`, by rejection sampling (arc4random_uniform
    /// style). A bare `% bound` would favor small values whenever `bound`
    /// doesn't divide 2^32. Rejected draws still come from the seeded stream,
    /// so the loop stays deterministic.
    fn below(&mut self, bound: NonZeroU32) -> u32 {
        let bound = bound.get();
        // 2^32 mod bound: the draws under this are the ones that would make
        // the low residues more likely.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let draw = self.next_u32();
            if draw >= threshold {
                return draw % bound;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Named imports: the prelude glob also exports a `Rng` trait.
    use proptest::prelude::{any, prop_assert, prop_assert_eq, prop_assume, proptest};
    use std::collections::BTreeSet;

    fn first_draws(seed: u32, count: usize) -> Vec<u32> {
        let mut rng = Rng::from_seed(seed);
        (0..count).map(|_| rng.next_u32()).collect()
    }

    #[test]
    fn next_u32_matches_reference_mulberry32() {
        // Tommy Ettinger's JS mulberry32, taking the u32 before the `/ 2^32`.
        assert_eq!(
            first_draws(0, 5),
            [1144304738, 1416247, 958946056, 627933444, 2007157716]
        );
        assert_eq!(
            first_draws(42, 5),
            [2581720956, 1925393290, 3661312704, 2876485805, 750819978]
        );
    }

    // There's no external reference for `below` and `shuffle`. These values
    // come from a Python port of the algorithms in the engine spec (E1).

    #[test]
    fn next_range_draws_are_pinned() {
        let mut rng = Rng::from_seed(42);
        let draws: Vec<Option<u32>> = (0..10).map(|_| rng.next_range(0..10)).collect();
        let expected = [6, 0, 4, 5, 8, 7, 0, 7, 0, 5].map(Some);
        assert_eq!(draws, expected);
    }

    #[test]
    fn shuffle_order_is_pinned() {
        let mut items: Vec<u32> = (0..10).collect();
        Rng::from_seed(42).shuffle(&mut items);
        assert_eq!(items, [1, 3, 9, 5, 2, 8, 4, 0, 7, 6]);
    }

    #[test]
    fn same_seed_replays_identically() {
        assert_eq!(first_draws(42, 1_000), first_draws(42, 1_000));
    }

    #[test]
    fn different_seeds_diverge() {
        let prefixes: BTreeSet<Vec<u32>> = (0..100).map(|seed| first_draws(seed, 16)).collect();
        assert_eq!(prefixes.len(), 100);
    }

    #[test]
    fn empty_range_is_none_and_draws_nothing() {
        let mut rng = Rng::from_seed(42);
        let (start, end) = (6, 5);
        assert_eq!(rng.next_range(5..5), None);
        assert_eq!(rng.next_range(start..end), None);
        assert_eq!(rng.next_u32(), first_draws(42, 1)[0]);
    }

    #[test]
    fn single_value_range_returns_its_start() {
        let mut rng = Rng::from_seed(0);
        for _ in 0..100 {
            assert_eq!(rng.next_range(7..8), Some(7));
        }
    }

    #[test]
    fn shuffle_leaves_tiny_slices_unchanged() {
        let mut rng = Rng::from_seed(0);
        let mut empty: [u32; 0] = [];
        rng.shuffle(&mut empty);
        let mut single = [9];
        rng.shuffle(&mut single);
        assert_eq!(single, [9]);
    }

    proptest! {
        #![proptest_config(crate::test_support::proptest_config())]

        #[test]
        fn next_range_stays_in_range(seed in any::<u32>(), a in any::<u32>(), b in any::<u32>()) {
            let (start, end) = (a.min(b), a.max(b));
            prop_assume!(start < end);
            let mut rng = Rng::from_seed(seed);
            for _ in 0..32 {
                let draw = rng.next_range(start..end);
                prop_assert!(draw.is_some_and(|value| (start..end).contains(&value)));
            }
        }

        #[test]
        fn shuffle_is_a_deterministic_permutation(seed in any::<u32>(), len in 0usize..50) {
            let original: Vec<usize> = (0..len).collect();
            let mut shuffled = original.clone();
            Rng::from_seed(seed).shuffle(&mut shuffled);
            let mut again = original.clone();
            Rng::from_seed(seed).shuffle(&mut again);
            prop_assert_eq!(&shuffled, &again);

            shuffled.sort_unstable();
            prop_assert_eq!(shuffled, original);
        }
    }
}

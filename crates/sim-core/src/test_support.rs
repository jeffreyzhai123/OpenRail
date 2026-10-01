use proptest::test_runner::{Config, RngSeed};

/// Fixed so property tests are as deterministic as the code they test
/// (CLAUDE.md). Change the seed deliberately to explore new inputs.
const PROPTEST_SEED: u64 = 0x5EED_5AFE;

pub fn proptest_config() -> Config {
    Config {
        rng_seed: RngSeed::Fixed(PROPTEST_SEED),
        ..Config::default()
    }
}

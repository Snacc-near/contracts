//! DK-026 — a `CrucibleTemp` property test against yocto supplies of 10^18..10^30:
//! every zone must be REACHABLE across the whole scale range (proportional activity
//! V = c×M yields the same temperature — a scale invariant).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so the test paths do not change
mod qa_dk026_tests {
    use crate::state::MacroState;

    /// A minimal MacroState for testing the pure formula (public fields).
    fn macro_with(volume: u128, supply: u128) -> MacroState {
        MacroState {
            volume_epoch: volume,
            total_snacc_supply: supply,
            ..Default::default()
        }
    }

    /// Property 1 — every zone is reached at ALL supply scales 1e18..1e30:
    /// V = 0.61×M → 610 mK (the Fever threshold), V = 0.30×M → 300 mK (Hypo),
    /// V = 0 → 0 mK (Hypo). Log-spaced scales 18..=30.
    #[test]
    fn tc_dk026_every_zone_reached_at_all_supply_scales() {
        for exp in 18u32..=30 {
            let supply = 10u128.pow(exp);
            // V = c% × M → temp = 1000 × c/100 = 10×c mK.
            for (c, expected_temp) in [(0u128, 0u64), (3, 30), (61, 610), (150, 1500)] {
                let volume = supply * c / 100;
                let m = macro_with(volume, supply);
                let temp = m.crucible_temp_at(0);
                assert_eq!(
                    temp, expected_temp,
                    "supply 1e{exp}: V={c}% should be {expected_temp} mK, got {temp}"
                );
            }
        }
    }

    /// Property 2 — a full scale invariant: two economies 6 orders of magnitude apart
    /// (within the tested 1e18..1e30 range) with identical activity proportions → identical temperature.
    #[test]
    fn tc_dk026_scale_invariant_two_economies() {
        for exp2 in 18u32..=24 {
            let small = macro_with(10u128.pow(exp2) * 61 / 100, 10u128.pow(exp2));
            let big = macro_with(10u128.pow(exp2 + 6) * 61 / 100, 10u128.pow(exp2 + 6));
            assert_eq!(
                small.crucible_temp_at(0),
                big.crucible_temp_at(0),
                "economies 1e{exp2} vs 1e{} (V 61%) should have identical temperatures",
                exp2 + 6
            );
        }
    }

    /// Property 3 — monotonic: the temperature never drops as volume rises (at any scale).
    #[test]
    fn tc_dk026_monotonic_in_volume() {
        let supply = 10u128.pow(24);
        let mut prev = 0u64;
        // V = c% × M for rising c → temp = 10×c mK, monotonic; the extreme case → clamp u64::MAX.
        for c in [0u128, 1, 10, 61, 150, 1_000] {
            let m = macro_with(supply * c / 100, supply);
            let t = m.crucible_temp_at(0);
            assert!(t >= prev, "temperature rises monotonically: {t} < {prev}");
            prev = t;
        }
        let m = macro_with(u128::MAX / 2, supply);
        assert!(m.crucible_temp_at(0) >= prev, "the clamp stays >= the previous temperature");
    }

    /// Property 4 — no u64/u128 wrap at extreme scales (a large V, a small supply).
    #[test]
    fn tc_dk026_no_wrap_at_extreme_scale() {
        let m = macro_with(u128::MAX / 2, 10u128.pow(18));
        let t = m.crucible_temp_at(0);
        assert_eq!(t, u64::MAX, "clamped to u64::MAX, no wrap");
    }
}

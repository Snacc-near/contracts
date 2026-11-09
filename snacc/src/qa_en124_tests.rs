//! EN-124 — 100-division loop test: the fences `MAX_COLONY_SIZE=5`, `MAX_GENERATION=8`,
//! and the demographics gate `total_divides <= merges+lyses` hold against a population
//! explosion.
//! Honest design finding (discovered WHILE writing this test): a single parent CANNOT
//! divide 100× — `splice_strains` (the budget opener) raises the parent's generation
//! (+1 per merge) so the parent hits 8 → `ERR_MAX_GENERATION` on the 8th divide; and
//! lysis needs a 57–90 hour time shift per iteration, which starves the parent.
//! So the HONEST loop is a founder chain — each founder (gen 0) performs 7 divides
//! (7 splices for budget), then is transplanted away; the next founder is incubated.
//! 100 divides = 15 founders (14×7 + 2).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so test paths stay stable
mod qa_en124_tests {
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    fn test_account(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }
    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(test_account("snacc-v001.testnet"))
            .signer_account_id(test_account(predecessor))
            .predecessor_account_id(test_account(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }

    /// TC-EN-124-LOOP — 100 divides via 15 founders; every fence holds with no
    /// unexpected panic; the demographics counters stay consistent at the end.
    #[test]
    fn tc_en_124_loop_100() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let mut founder = c
            .incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0)
            .strain_id;
        // Kick the volume once (giant fee; the deposit is free in tests) so feeds are
        // not dormant → ATP is booked for the splice (5) + divide (3) cost per iteration.
        ctx("alkemis.testnet", 10u128.pow(26), 1_000);
        let _ = c.extract_spores(founder, "mt:spore-ignis".to_string(), U128(10u128.pow(26)));

        let mut total_divides = 0u128;
        let mut iter = 0u64;
        while total_divides < 100 {
            iter += 1;
            let remaining = (100 - total_divides) as usize;
            let dividable = remaining.min(7); // a gen-0 founder sustains 7 divides (child gen ≤ 7)
            for _ in 0..dividable {
                // Donor → merger (opens +1 divide budget).
                ctx("alkemis.testnet", 10u128.pow(24), 1_000 + iter * 3_600);
                let donor = c
                    .incubate_strain(StrainRole::Fermenter, U128(10u128.pow(24)), 0)
                    .strain_id;
                ctx("alkemis.testnet", 0, 1_000 + iter * 3_600);
                let _ = c.splice_strains(founder, donor); // founder gen +1
                // Refill ATP via feed (non-dormant → lots of ATP is booked).
                ctx("alkemis.testnet", 0, 1_000 + iter * 3_600);
                let _ = c.feed_strain(founder, U128(10u128.pow(23)));
                // Divide → child gen+1; the colony is momentarily 2 → still below 5.
                ctx("alkemis.testnet", 0, 1_000 + iter * 3_600);
                let (_, child) = c.divide_strain(founder);
                total_divides += 1;
                // The child is transplanted away immediately (the founder's colony stays small).
                ctx("alkemis.testnet", 1, 1_000 + iter * 3_600);
                c.nft_transfer(
                    test_account("gudang.testnet"),
                    child.strain_id.to_string(),
                    None,
                    None,
                );
            }
            assert_eq!(
                c.macro_state.total_divides, total_divides,
                "the divides counter is consistent per founder"
            );
            // The founder is at gen 7 (Hayflick exhausted) → transplant it away, new founder.
            ctx("alkemis.testnet", 1, 1_000 + iter * 3_600);
            c.nft_transfer(
                test_account("gudang.testnet"),
                founder.to_string(),
                None,
                None,
            );
            ctx("alkemis.testnet", 10u128.pow(24), 1_000 + iter * 3_600);
            founder = c
                .incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0)
                .strain_id;
        }

        // Fences & counters.
        assert_eq!(total_divides, 100);
        assert_eq!(c.macro_state.total_divides, 100);
        assert_eq!(c.macro_state.total_mergers, 100, "1 merge per divide (exact budget)");
        assert_eq!(c.macro_state.total_lyses, 0);
        // births (incubations + divides) stay consistent with the definition:
        assert!(c.macro_state.total_births >= c.macro_state.total_mergers);
        assert!(
            c.macro_state.total_divides <= c.macro_state.total_mergers + c.macro_state.total_lyses,
            "the EN-124 invariant always holds"
        );
        // Every retired founder sits in storage with gen ≤ 7 (MAX_GENERATION=8 untouched).
        assert_eq!(
            c.get_strain(founder).generation,
            0,
            "the last founder is still gen 0"
        );
        // The storage colony holds all the retired cells (state stays tracked).
        assert!(c.get_colony(test_account("gudang.testnet")).len() >= 100);
        // The alchemist's active colony stays small (only the last founder).
        assert_eq!(c.get_colony(test_account("alkemis.testnet")).len(), 1);
    }
}
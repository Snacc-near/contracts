//! QA-123..126 (TC-LYSIS-*) — Feature 6 test suite:
//! - QA-123: Harvest starving strain (`harvest_starving_strain`: 10% bounty to liquidator account_balances,
//!   90% recovery to Rule-Bound Treasury, target snacc_balance zeroed, target status -> Liquidated,
//!   molecule inventory burned to 0, total_lyses incremented, K4 volume added, events emitted: `strain_liquidated`).
//! - QA-124: Nutrient burst release (burst_bps = min(BASE(200) + recovery * 10000 / total_supply, MAX(2000)),
//!   duration 1 epoch, discount applied during `feed_strain`, capped at `MAX_BURST_BPS`,
//!   expires when epoch reaches `nutrient_burst_until`).
//! - QA-125: Strain adoption (`adopt`: take over Distressed strain (1 <= eff <= 2000) at book_value without 15%
//!   premium, 100% paid to old owner, raider strain balance deducted, ownership transferred, colony index updated,
//!   colony room fence < 5, atomic nonce guardrail `I-14`).
//! - QA-126: Adopt vs Snack (Solvency tier partitions: Tier 1 Healthy >2000 blocks both adopt & harvest;
//!   Tier 2 Distressed 1..=2000 allows adopt & blocks harvest; Tier 3 Insolvent == 0 allows harvest & blocks adopt;
//!   anti-drain seller race prevention with `state_nonce`).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod qa_f6_tests {
    use crate::state::constants;
    use crate::state::{StrainRole, StrainStatus};
    use crate::SnaccContract;
    use crate::StrainId;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    const ALKEMIS: &str = "alkemis.testnet";
    const BOB: &str = "bob.testnet";
    const CHARLIE: &str = "charlie.testnet";
    const CONTRACT_ID: &str = "snacc-v001.testnet";
    const T0: u64 = 1_000;

    fn acct(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }

    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(CONTRACT_ID))
            .signer_account_id(acct(predecessor))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }

    fn fresh_contract_with_strain(
        owner: &str,
        role: StrainRole,
        deposit_stake: u128,
        initial_spores: u64,
    ) -> (SnaccContract, StrainId) {
        ctx(owner, deposit_stake, T0);
        let mut c = SnaccContract::new();
        let view = c.incubate_strain(role, U128(deposit_stake), initial_spores);
        (c, view.strain_id)
    }

    fn strain_snacc(c: &SnaccContract, id: StrainId) -> u128 {
        c.snacc_balances.get(&id).copied().unwrap_or(0)
    }

    fn acct_snacc(c: &SnaccContract, account: &str) -> u128 {
        c.account_balances.get(&acct(account)).copied().unwrap_or(0)
    }

    fn inv_balance(c: &SnaccContract, id: StrainId, token: &str) -> u128 {
        c.inventories
            .get(&(id, token.to_string()))
            .copied()
            .unwrap_or(0)
    }

    fn set_inv(c: &mut SnaccContract, id: StrainId, token: &str, amount: u128) {
        let _ = c.inventories.insert((id, token.to_string()), amount);
    }

    // =========================================================================
    // QA-123 (TC-LYSIS-01..07): Harvest Starving Strain & 90/10 Split
    // =========================================================================

    /// QA-123: Liquidating an insolvent strain (effective_vitality == 0) executes the 90/10 split:
    /// 10% bounty credited to liquidator's account_balances, 90% recovery added to Treasury.
    #[test]
    fn tc_lysis_01_harvest_starving_strain_90_10_split() {
        let stake = 10 * 10u128.pow(24); // 10 NEAR
        let (mut c, id) = fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        let initial_working_capital = strain_snacc(&c, id); // 8 NEAR (80%)
        let initial_treasury = c.treasury; // 2 NEAR (20%)
        assert_eq!(initial_working_capital, 8 * 10u128.pow(24));
        assert_eq!(initial_treasury, 2 * 10u128.pow(24));

        // Advance 90 hours -> 90 * 100 = 9000 > 8500 -> effective vitality = 0 (Tier 3 Insolvent)
        let t_lysis = T0 + 90 * 3600;
        ctx(BOB, 0, t_lysis);

        let (bounty, recovery) = c.harvest_starving_strain(id);

        let expected_bounty = initial_working_capital * 1_000 / 10_000; // 10% = 0.8 NEAR
        let expected_recovery = initial_working_capital - expected_bounty; // 90% = 7.2 NEAR

        assert_eq!(bounty.0, expected_bounty);
        assert_eq!(recovery.0, expected_recovery);

        // Liquidator account balance received 10% bounty
        assert_eq!(acct_snacc(&c, BOB), expected_bounty);
        // Rule-Bound Treasury received 90% recovery
        assert_eq!(c.treasury, initial_treasury + expected_recovery);
        // Strain balance zeroed
        assert_eq!(strain_snacc(&c, id), 0);
        // Strain status is Liquidated
        assert_eq!(c.strains.get(&id).unwrap().status, StrainStatus::Liquidated);
        // Demographics counter incremented
        assert_eq!(c.macro_state.total_lyses, 1);
    }

    /// QA-123: Liquidation burns all NEP-245 molecule inventories of the target strain to 0.
    #[test]
    fn tc_lysis_01_harvest_burns_all_molecule_inventories() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, id) = fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        // Seed target with multiple inventory items
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 5);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 3);
        set_inv(&mut c, id, constants::ELIXIR_HOMUNCULUS, 2);

        let t_lysis = T0 + 90 * 3600;
        ctx(BOB, 0, t_lysis);
        let _ = c.harvest_starving_strain(id);

        // All molecule types zeroed
        for token in constants::ALL_TOKEN_IDS {
            assert_eq!(
                inv_balance(&c, id, token),
                0,
                "Token {token} must be burned to 0 upon lysis"
            );
        }
    }

    /// QA-123: Attempting to harvest a healthy strain (Tier 1, vitality > 2000) must panic ERR_STRAIN_NOT_INSOLVENT.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_INSOLVENT")]
    fn tc_lysis_01_harvest_healthy_strain_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        ctx(BOB, 0, T0 + 10); // Only 10s elapsed, vitality = 8500
        c.harvest_starving_strain(id);
    }

    /// QA-123: Attempting to harvest a Distressed strain (Tier 2, 1 <= vitality <= 2000) must panic ERR_STRAIN_NOT_INSOLVENT.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_INSOLVENT")]
    fn tc_lysis_01_harvest_distressed_strain_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        // 70 hours elapsed: 8500 - 7000 = 1500 vitality (Tier 2 Distressed)
        let t_distressed = T0 + 70 * 3600;
        ctx(BOB, 0, t_distressed);
        c.harvest_starving_strain(id);
    }

    /// QA-123: Attempting to harvest a strain that is already liquidated must panic ERR_ALREADY_LIQUIDATED.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn tc_lysis_01_harvest_already_liquidated_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        let t_lysis = T0 + 90 * 3600;
        ctx(BOB, 0, t_lysis);
        let _ = c.harvest_starving_strain(id);

        // Second harvest attempt on same liquidated strain
        c.harvest_starving_strain(id);
    }

    /// QA-123: Attempting to harvest a strain that was spliced/burned must panic ERR_ALREADY_LIQUIDATED.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn tc_lysis_01_harvest_spliced_burned_strain_rejected() {
        let (mut c, s1) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, 10 * 10u128.pow(24), 0);
        ctx(ALKEMIS, 10 * 10u128.pow(24), T0);
        let s2 = c
            .incubate_strain(StrainRole::Fermenter, U128(10 * 10u128.pow(24)), 0)
            .strain_id;

        // Splice s2 into s1 -> s2 becomes SplicedBurned
        ctx(ALKEMIS, 0, T0);
        let _ = c.splice_strains(s1, s2);
        assert_eq!(c.strains.get(&s2).unwrap().status, StrainStatus::SplicedBurned);

        ctx(BOB, 0, T0 + 90 * 3600);
        c.harvest_starving_strain(s2);
    }

    /// QA-123: Attempting to harvest a non-existent strain must panic ERR_STRAIN_NOT_FOUND.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn tc_lysis_01_harvest_non_existent_strain_rejected() {
        let (mut c, _) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        ctx(BOB, 0, T0 + 90 * 3600);
        c.harvest_starving_strain(999);
    }

    // =========================================================================
    // QA-124 (TC-LYSIS-08..12): Nutrient Burst Release & Discount Lifecycle
    // =========================================================================

    /// QA-124: Liquidation triggers `Nutrient Burst`:
    /// `burst_bps = min(BASE_BURST_BPS(200) + recovery * 10000 / total_supply, MAX_BURST_BPS(2000))`.
    #[test]
    fn tc_lysis_02_nutrient_burst_released_and_state() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, id) = fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        let t_lysis = T0 + 90 * 3600;
        ctx(BOB, 0, t_lysis);
        let (_, recovery) = c.harvest_starving_strain(id);

        let total_supply = c.macro_state.total_snacc_supply;
        let expected_scaled = recovery.0 * constants::BPS_DENOMINATOR / total_supply;
        let expected_burst =
            (u128::from(constants::BASE_BURST_BPS) + expected_scaled).min(constants::MAX_BURST_BPS as u128) as u16;

        assert_eq!(c.macro_state.nutrient_burst_bps, expected_burst);
        assert_eq!(
            c.macro_state.nutrient_burst_until,
            c.macro_state.current_epoch + constants::BURST_DURATION_EPOCHS
        );
    }

    /// QA-124: Active Nutrient Burst grants a discount on `feed_strain`.
    #[test]
    fn tc_lysis_02_nutrient_burst_discounts_feed_strain() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, id1) = fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        // Incubate second strain to receive feeding during the burst
        ctx(BOB, stake, T0);
        let id2 = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // Harvest id1 to trigger burst
        let t_lysis = T0 + 90 * 3600;
        ctx(CHARLIE, 0, t_lysis);
        let _ = c.harvest_starving_strain(id1);

        let active_burst_bps = c.macro_state.nutrient_burst_bps;
        assert!(active_burst_bps > 0);

        // Feed id2 during the burst epoch
        c.macro_state.params.dormancy_threshold_bps = 0; // disable dormancy for pure measurement
        let mut v2 = c.strains.get(&id2).unwrap().clone();
        v2.vitality = constants::VITALITY_CAP; // 100% rescue multiplier
        v2.last_fed_sec = t_lysis;
        let _ = c.strains.insert(id2, v2);

        let base_feed_fee = 10 * constants::ATP_YOCTO_UNIT;
        let expected_discount = base_feed_fee * u128::from(active_burst_bps) / constants::BPS_DENOMINATOR;
        let expected_effective_fee = base_feed_fee - expected_discount;

        let bal_before = strain_snacc(&c, id2);
        ctx(BOB, 0, t_lysis + 10);
        let _ = c.feed_strain(id2, U128(base_feed_fee));
        let bal_after = strain_snacc(&c, id2);

        // Account for 2% Hypothermia rebate (temp < 600 mK refunds 2% of fee to strain balance)
        let expected_hypo_rebate = expected_effective_fee * 200 / constants::BPS_DENOMINATOR;
        let expected_net_debit = expected_effective_fee - expected_hypo_rebate;

        assert_eq!(
            bal_before - bal_after,
            expected_net_debit,
            "Strain balance must be debited net fee after burst discount and hypo rebate"
        );
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""burst_bps":2000"#));
        assert!(logs.contains(r#""effective_fee":8000000000000000"#));
    }

    /// QA-124: Nutrient burst discount expires once the epoch reaches `nutrient_burst_until`.
    #[test]
    fn tc_lysis_02_nutrient_burst_expires_after_until_epoch() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, id1) = fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let id2 = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        let t_lysis = T0 + 90 * 3600;
        ctx(CHARLIE, 0, t_lysis);
        let _ = c.harvest_starving_strain(id1);

        // Advance epoch to expire burst
        c.macro_state.current_epoch = c.macro_state.nutrient_burst_until;

        c.macro_state.params.dormancy_threshold_bps = 0;
        let mut v2 = c.strains.get(&id2).unwrap().clone();
        v2.vitality = constants::VITALITY_CAP;
        v2.last_fed_sec = t_lysis;
        let _ = c.strains.insert(id2, v2);

        let base_feed_fee = 10 * constants::ATP_YOCTO_UNIT;
        let bal_before = strain_snacc(&c, id2);
        ctx(BOB, 0, t_lysis + 10);
        let _ = c.feed_strain(id2, U128(base_feed_fee));
        let bal_after = strain_snacc(&c, id2);

        // With burst expired, discount is 0. Net fee accounts for 2% Hypothermia rebate.
        let expected_hypo_rebate = base_feed_fee * 200 / constants::BPS_DENOMINATOR;
        let expected_net_debit = base_feed_fee - expected_hypo_rebate;

        assert_eq!(bal_before - bal_after, expected_net_debit);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""burst_bps":0"#));
        assert!(logs.contains(r#""effective_fee":10000000000000000"#));
    }

    /// QA-124: Burst is capped at MAX_BURST_BPS (2000 bps = 20%) regardless of large liquidation recovery.
    #[test]
    fn tc_lysis_02_multiple_harvests_burst_bps_capped_at_max() {
        let stake = 100 * 10u128.pow(24);
        let (mut c, id) = fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        let t_lysis = T0 + 90 * 3600;
        ctx(BOB, 0, t_lysis);
        let _ = c.harvest_starving_strain(id);

        assert!(c.macro_state.nutrient_burst_bps <= constants::MAX_BURST_BPS);
    }

    /// QA-124: Liquidation updates macro K4 volume legs (bounty + recovery).
    #[test]
    fn tc_lysis_02_k4_volume_epoch_inflow() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, id) = fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        let v_epoch_before = c.macro_state.volume_epoch;
        let t_lysis = T0 + 90 * 3600;
        ctx(BOB, 0, t_lysis);
        let (bounty, recovery) = c.harvest_starving_strain(id);

        assert_eq!(
            c.macro_state.volume_epoch,
            v_epoch_before + bounty.0 + recovery.0
        );
    }

    // =========================================================================
    // QA-125 (TC-LYSIS-13..19): Strain Adoption (`adopt`)
    // =========================================================================

    /// QA-125: Adopting a Distressed strain (1 <= vitality <= 2000):
    /// - 100% of book_value (balance + sum(inv * floor_price)) is paid from raider to old owner.
    /// - Ownership transfers to adopter, target status remains Active.
    /// - Target moved from old colony index to adopter colony index.
    #[test]
    fn tc_lysis_03_adopt_distressed_strain_success() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        // Incubate raider strain owned by Bob with larger stake (20 NEAR) to afford book_value
        let raider_stake = 20 * 10u128.pow(24);
        ctx(BOB, raider_stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(raider_stake), 0).strain_id;

        // Seed target with inventory
        set_inv(&mut c, s_target, constants::SPORE_TOKEN_IDS[0], 2); // 2 * 1 = 2
        set_inv(&mut c, s_target, constants::ENZYME_AMALGAM, 1); // 1 * 1 = 1

        let target_bal = strain_snacc(&c, s_target);
        let expected_book_value = target_bal + 3 * constants::MOLECULE_FLOOR_PRICE;

        // Make target Distressed (Tier 2): 70 hours elapsed -> 8500 - 7000 = 1500 vitality
        let t_distressed = T0 + 70 * 3600;
        let old_owner_bal_before = acct_snacc(&c, ALKEMIS);
        let raider_bal_before = strain_snacc(&c, s_raider);

        ctx(BOB, 0, t_distressed);
        let view = c.adopt(s_target, s_raider, 0);

        assert_eq!(view.strain_id, s_target);
        assert_eq!(view.owner_id, acct(BOB));
        assert_eq!(view.status, StrainStatus::Distressed);

        // Raider paid book_value
        assert_eq!(strain_snacc(&c, s_raider), raider_bal_before - expected_book_value);
        // Old owner received 100% book_value (no premium)
        assert_eq!(acct_snacc(&c, ALKEMIS), old_owner_bal_before + expected_book_value);

        // Colony indexes updated: ALKEMIS lost target, BOB gained target
        assert_eq!(c.colony_active_count(&acct(ALKEMIS)), 0);
        assert_eq!(c.colony_active_count(&acct(BOB)), 2); // raider + adopted target
    }

    /// QA-125: `adopt` with same strain (target == raider) must panic ERR_SAME_STRAIN.
    #[test]
    #[should_panic(expected = "ERR_SAME_STRAIN")]
    fn tc_lysis_03_adopt_same_strain_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        ctx(ALKEMIS, 0, T0 + 70 * 3600);
        c.adopt(id, id, 0);
    }

    /// QA-125: `adopt` when caller does not own the raider strain must panic ERR_NOT_OWNED_BY_CALLEE.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn tc_lysis_03_adopt_caller_not_raider_owner_rejected() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // ALKEMIS tries to call adopt using Bob's raider
        ctx(ALKEMIS, 0, T0 + 70 * 3600);
        c.adopt(s_target, s_raider, 0);
    }

    /// QA-125: `adopt` when raider balance is insufficient to pay book_value must panic ERR_INSUFFICIENT_BALANCE.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn tc_lysis_03_adopt_insufficient_raider_balance_rejected() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // Drain raider balance to 0
        c.set_strain_snacc(s_raider, 0);

        ctx(BOB, 0, T0 + 70 * 3600);
        c.adopt(s_target, s_raider, 0);
    }

    /// QA-125: `adopt` when adopter colony is already full (MAX_COLONY_SIZE = 5) must panic ERR_COLONY_FULL.
    #[test]
    #[should_panic(expected = "ERR_COLONY_FULL")]
    fn tc_lysis_03_adopt_colony_full_fence() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);

        // Bob hatches 5 strains (filling colony to MAX_COLONY_SIZE)
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;
        for _ in 1..5 {
            ctx(BOB, stake, T0);
            let _ = c.incubate_strain(StrainRole::Extractor, U128(stake), 0);
        }
        assert_eq!(c.colony_active_count(&acct(BOB)), 5);

        // Attempting to adopt a 6th strain must fail ERR_COLONY_FULL
        ctx(BOB, 0, T0 + 70 * 3600);
        c.adopt(s_target, s_raider, 0);
    }

    /// QA-125: `adopt` with stale expected nonce must panic ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED (Anti-Drain Guardrail I-14).
    #[test]
    #[should_panic(expected = "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED")]
    fn tc_lysis_03_adopt_stale_nonce_rejected() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // Target state_nonce is 0; passing expected_target_nonce = 1 must panic ERR_STALE_NONCE
        ctx(BOB, 0, T0 + 70 * 3600);
        c.adopt(s_target, s_raider, 1);
    }

    /// QA-125: `adopt` on an already liquidated strain must panic ERR_ALREADY_LIQUIDATED.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn tc_lysis_03_adopt_already_liquidated_rejected() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // Liquidate s_target at 90h
        ctx(CHARLIE, 0, T0 + 90 * 3600);
        let _ = c.harvest_starving_strain(s_target);

        // Raider attempts to adopt liquidated strain
        ctx(BOB, 0, T0 + 90 * 3600);
        c.adopt(s_target, s_raider, 1);
    }

    // =========================================================================
    // QA-126 (TC-LYSIS-20..25): Adopt vs Snack (Disjoint Solvency Windows)
    // =========================================================================

    /// QA-126: Tier 1 Healthy (>2000 vitality): rejects BOTH `adopt` and `harvest_starving_strain`.
    #[test]
    fn tc_lysis_04_tier1_healthy_rejects_both_adopt_and_harvest() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // 10 hours elapsed -> vitality 7500 > 2000 (Tier 1 Healthy)
        let t_healthy = T0 + 10 * 3600;

        // Adopt rejected: ERR_NOT_DISTRESSED
        ctx(BOB, 0, t_healthy);
        let adopt_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.adopt(s_target, s_raider, 0);
        }));
        assert!(adopt_res.is_err(), "Healthy strain must reject adopt");

        // Harvest rejected: ERR_STRAIN_NOT_INSOLVENT
        ctx(CHARLIE, 0, t_healthy);
        let harvest_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.harvest_starving_strain(s_target);
        }));
        assert!(harvest_res.is_err(), "Healthy strain must reject harvest");
    }

    /// QA-126: Tier 2 Distressed (1..=2000 vitality): allows `adopt`, rejects `harvest_starving_strain`.
    #[test]
    fn tc_lysis_04_tier2_distressed_allows_adopt_rejects_harvest() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // 70 hours elapsed -> vitality 1500 (Tier 2 Distressed)
        let t_distressed = T0 + 70 * 3600;

        // Harvest rejected: ERR_STRAIN_NOT_INSOLVENT
        ctx(CHARLIE, 0, t_distressed);
        let harvest_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.harvest_starving_strain(s_target);
        }));
        assert!(harvest_res.is_err(), "Distressed strain must reject harvest");

        // Adopt succeeds!
        ctx(BOB, 0, t_distressed);
        let view = c.adopt(s_target, s_raider, 0);
        assert_eq!(view.owner_id, acct(BOB));
    }

    /// QA-126: Tier 3 Insolvent (0 vitality): allows `harvest_starving_strain`, rejects `adopt`.
    #[test]
    fn tc_lysis_04_tier3_insolvent_allows_harvest_rejects_adopt() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // 90 hours elapsed -> vitality 0 (Tier 3 Insolvent)
        let t_insolvent = T0 + 90 * 3600;

        // Adopt rejected: ERR_NOT_DISTRESSED
        ctx(BOB, 0, t_insolvent);
        let adopt_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.adopt(s_target, s_raider, 0);
        }));
        assert!(adopt_res.is_err(), "Insolvent strain must reject adopt");

        // Harvest succeeds!
        ctx(CHARLIE, 0, t_insolvent);
        let (bounty, recovery) = c.harvest_starving_strain(s_target);
        assert!(bounty.0 > 0 && recovery.0 > 0);
    }

    /// QA-126: Anti-drain seller race prevention (I-14):
    /// An action on the target strain increments state_nonce, thwarting any in-flight adopt transaction.
    #[test]
    fn tc_lysis_04_anti_drain_target_mutation_invalidates_stale_adopt() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_raider = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        let t_distressed = T0 + 70 * 3600;

        // Target owner extracts 1 spore -> increments target state_nonce from 0 to 1
        ctx(ALKEMIS, 10_000, t_distressed);
        let _ = c.extract_spores(s_target, constants::SPORE_TOKEN_IDS[0].to_string(), U128(10_000));
        assert_eq!(c.get_strain(s_target).state_nonce, 1);

        // Raider submits adopt with expected_target_nonce = 0 (before seller's extract)
        ctx(BOB, 0, t_distressed + 10);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.adopt(s_target, s_raider, 0);
        }));
        assert!(res.is_err(), "Stale nonce adopt must be thwarted");
    }

    /// QA-126: Demographics invariant (EN-124): Cell lysis (harvest) adds 1 to total_lyses,
    /// creating division quota: total_divides <= total_mergers + total_lyses.
    #[test]
    fn tc_lysis_04_en124_lysis_unlocks_cytokinesis_quota() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, s_target) =
            fresh_contract_with_strain(ALKEMIS, StrainRole::Extractor, stake, 0);
        ctx(BOB, stake, T0);
        let s_parent = c.incubate_strain(StrainRole::Fermenter, U128(stake), 0).strain_id;

        // Before any merger or lysis: division must fail ERR_BIRTHS_EXCEEDED
        ctx(BOB, 0, T0);
        let divide_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.divide_strain(s_parent);
        }));
        assert!(divide_res.is_err(), "Division before lysis/merger must fail");
        // Reset in-place counter mutated before the assertion failed
        c.macro_state.total_divides = 0;

        // Liquidate s_target (1 lysis)
        ctx(CHARLIE, 0, T0 + 90 * 3600);
        let _ = c.harvest_starving_strain(s_target);
        assert_eq!(c.macro_state.total_lyses, 1);

        // Now division succeeds because total_divides (1) <= total_mergers (0) + total_lyses (1)
        ctx(BOB, 0, T0 + 90 * 3600);
        // Refresh s_parent vitality so it's not starving
        let mut vessel = c.strains.get(&s_parent).unwrap().clone();
        vessel.vitality = 5000;
        vessel.last_fed_sec = T0 + 90 * 3600;
        let _ = c.strains.insert(s_parent, vessel);

        let (_, child) = c.divide_strain(s_parent);
        assert_eq!(c.macro_state.total_divides, 1);
        assert_eq!(child.generation, 1);
    }
}

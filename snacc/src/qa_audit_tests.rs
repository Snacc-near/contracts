//! Contract-fix test suite from the 2026-11-08 completeness audit (W4–W10).
//! Self-contained (local `ctx`/`fresh_contract` helpers — the qa_f1_tests pattern).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*
mod qa_audit_tests {
    use crate::state::constants;
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use crate::StrainId;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    const OWNER: &str = "alkemis.testnet";
    const OTHER: &str = "intruder.testnet";
    const T0: u64 = 1_000;

    fn acct(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }
    fn ctx_on(contract: &str, predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(contract))
            .signer_account_id(acct(predecessor))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }
    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        ctx_on("snacc-v001.testnet", predecessor, deposit_yocto, ts_sec);
    }
    /// A fresh contract (owner = `OWNER`) + one NEAR-staked strain.
    fn fresh_contract(stake: u128, role: StrainRole, spores: u64) -> (SnaccContract, StrainId) {
        ctx(OWNER, stake, T0);
        let mut c = SnaccContract::new();
        let v = c.incubate_strain(role, U128(stake), spores);
        (c, v.strain_id)
    }

    // ------------------------------------------------------------------
    // W9 — `advance_sandbox_clock` (ADR-22) + the single `now_sec()`
    // ------------------------------------------------------------------

    /// The owner advances the clock 48 hours → BMR/aging follow (Extractor 100/h).
    #[test]
    fn w9_sandbox_clock_advances_aging() {
        let (mut c, id) = fresh_contract(10_000, StrainRole::Extractor, 0);
        assert!(c.sandbox_clock_enabled, "a .testnet account = sandbox mode");
        assert_eq!(c.get_strain(id).effective_vitality, 8500);
        ctx(OWNER, 0, T0);
        assert_eq!(c.advance_sandbox_clock(48), 48 * 3600);
        assert_eq!(c.get_strain(id).effective_vitality, 8500 - 48 * 100);
        let (enabled, offset, now) = c.get_sandbox_clock();
        assert!(enabled);
        assert_eq!(offset, 48 * 3600);
        assert_eq!(now, T0 + 48 * 3600);
    }

    /// Tier 2 → hostile opens, Tier 3 → lysis opens without waiting for real time.
    #[test]
    fn w9_sandbox_clock_opens_tier2_and_lysis() {
        let (mut c, id) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(70); // 8500 - 7000 = 1500 → Tier 2
        assert_eq!(c.get_strain(id).effective_vitality, 1500);
        let _ = c.advance_sandbox_clock(15); // → 0 → Tier 3
        assert_eq!(c.get_strain(id).effective_vitality, 0);
        ctx(OTHER, 0, T0);
        let (bounty, _) = c.harvest_starving_strain(id);
        assert!(bounty.0 > 0);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_CONTRACT_OWNER")]
    fn w9_sandbox_clock_non_owner_rejected() {
        let (mut c, _) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OTHER, 0, T0);
        let _ = c.advance_sandbox_clock(1);
    }

    /// A mainnet account (`.near`) → the flag is off since init; no setter.
    #[test]
    #[should_panic(expected = "ERR_SANDBOX_CLOCK_DISABLED")]
    fn w9_sandbox_clock_off_on_mainnet() {
        ctx_on("snacc-v001.near", OWNER, 0, T0);
        let mut c = SnaccContract::new();
        assert!(!c.sandbox_clock_enabled);
        let _ = c.advance_sandbox_clock(1);
    }

    /// `migrate()` on a mainnet account also turns the flag off.
    #[test]
    fn w9_migrate_mainnet_flag_off_testnet_on() {
        ctx_on("snacc-v001.near", "snacc-v001.near", 0, T0);
        assert!(!SnaccContract::migrate().sandbox_clock_enabled);
        ctx_on("snacc-v001.testnet", "snacc-v001.testnet", 0, T0);
        let m = SnaccContract::migrate();
        assert!(m.sandbox_clock_enabled);
        assert_eq!(m.sandbox_time_offset_sec, 0);
    }

    #[test]
    #[should_panic(expected = "ERR_BAD_AMOUNT")]
    fn w9_sandbox_clock_zero_hours_rejected() {
        let (mut c, _) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(0);
    }

    #[test]
    #[should_panic(expected = "ERR_BAD_AMOUNT")]
    fn w9_sandbox_clock_over_limit_rejected() {
        let (mut c, _) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(constants::MAX_SANDBOX_ADVANCE_HOURS + 1);
    }

    // ------------------------------------------------------------------
    // W4 — owner checks on the production paths (V-01, ERR_NOT_STRAIN_OWNER)
    // ------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn w4_extract_non_owner_rejected() {
        let (mut c, id) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OTHER, 0, T0);
        let _ = c.extract_spores(id, "mt:spore-ignis".to_string(), U128(0));
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn w4_transmute_non_owner_rejected() {
        let (mut c, id) = fresh_contract(10_000, StrainRole::Fermenter, 0);
        let _ = c.inventories.insert((id, "mt:spore-ignis".to_string()), 4);
        ctx(OTHER, 0, T0);
        let _ = c.transmute_spores_to_enzyme(id, 2, U128(0));
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn w4_synthesize_non_owner_rejected() {
        let (mut c, id) = fresh_contract(10_000, StrainRole::Synthesizer, 0);
        let _ = c.inventories.insert((id, constants::ENZYME_AMALGAM.to_string()), 4);
        let _ = c.inventories.insert((id, constants::ENZYME_SYNTHASE.to_string()), 4);
        ctx(OTHER, 0, T0);
        let _ = c.synthesize_elixir(id, constants::ELIXIR_HOMUNCULUS.to_string(), 3, U128(0));
    }

    /// The owner still can (positive regression) and the victim's ATP is untouched by other accounts.
    #[test]
    fn w4_owner_still_can_extract() {
        let (mut c, id) = fresh_contract(10_000, StrainRole::Extractor, 0);
        let atp0 = c.get_strain(id).atp_balance;
        ctx(OWNER, 0, T0);
        let v = c.extract_spores(id, "mt:spore-ignis".to_string(), U128(0));
        assert_eq!(v.atp_balance, atp0 - 1);
    }

    // ------------------------------------------------------------------
    // W5 — close the free mint of `incubate_strain` (I-01) + W7 onboarding
    // ------------------------------------------------------------------

    /// Deposit 0 + stake N without an account balance → rejected; supply unchanged.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn w5_stake_without_deposit_without_balance_rejected() {
        let (mut c, _) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OTHER, 0, T0);
        let _ = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0);
    }

    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn w5_stake_zero_deposit_zero_rejected() {
        ctx(OWNER, 0, T0);
        let mut c = SnaccContract::new();
        let _ = c.incubate_strain(StrainRole::Extractor, U128(0), 0);
    }

    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn w5_deposit_differs_from_stake_rejected() {
        ctx(OWNER, 5_000, T0);
        let mut c = SnaccContract::new();
        let _ = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
    }

    /// W7 — a new player only holds NEAR: hatching with a deposit = the onboarding path;
    /// the born supply = the deposit, 80% to the strain, can immediately `feed_strain`; V-04 solvent.
    #[test]
    fn w7_onboarding_new_player_with_near() {
        ctx(OWNER, 0, T0);
        let mut c = SnaccContract::new();
        let stake = 10u128.pow(24);
        ctx("new-player.testnet", stake, T0);
        let v = c.incubate_strain(StrainRole::Fermenter, U128(0), 2);
        assert_eq!(c.ft_total_supply().0, stake);
        assert_eq!(c.snacc_balances.get(&v.strain_id).copied(), Some(stake * 8 / 10));
        ctx("new-player.testnet", 0, T0);
        let _ = c.feed_strain(v.strain_id, U128(10u128.pow(21)));
        assert!(c.get_global_solvency().solvent);
    }

    /// A stake from the account balance (a lysis bounty) → the supply does NOT grow; the account
    /// balance is debited; the V-04 invariant stays solvent on every path.
    #[test]
    fn w5_stake_from_account_balance_without_mint() {
        let (mut c, id) = fresh_contract(10u128.pow(24), StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(100); // eff 0 → Tier 3
        ctx(OTHER, 0, T0);
        let (bounty, _) = c.harvest_starving_strain(id);
        assert!(bounty.0 > 0);
        assert_eq!(c.ft_balance_of(acct(OTHER)), bounty);
        let supply0 = c.ft_total_supply().0;
        assert!(c.get_global_solvency().solvent);

        let v = c.incubate_strain(StrainRole::Synthesizer, bounty, 0);
        assert_eq!(c.ft_total_supply().0, supply0, "no new mint");
        assert_eq!(c.ft_balance_of(acct(OTHER)).0, 0);
        let to_treasury = bounty.0 * 2000 / 10_000;
        assert_eq!(
            c.snacc_balances.get(&v.strain_id).copied(),
            Some(bounty.0 - to_treasury)
        );
        assert!(c.get_global_solvency().solvent);
    }

    // ------------------------------------------------------------------
    // W6 — the `advance_epoch` time gate
    // ------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "ERR_EPOCH_NOT_ENDED")]
    fn w6_advance_epoch_before_end_rejected() {
        let (mut c, _) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OTHER, 0, T0 + constants::EPOCH_DURATION_SEC - 1);
        c.advance_epoch();
    }

    /// Exactly at the boundary → passes; a second call in the same block → rejected (anti-spam).
    #[test]
    fn w6_advance_epoch_after_end_then_spam_rejected() {
        let (mut c, _) = fresh_contract(10_000, StrainRole::Extractor, 0);
        let t1 = T0 + constants::EPOCH_DURATION_SEC;
        ctx(OTHER, 0, t1);
        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.macro_state.epoch_ends_sec, t1 + constants::EPOCH_DURATION_SEC);
        let spam = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.advance_epoch()));
        assert!(spam.is_err());
    }

    /// The sandbox clock (ADR-22) also opens the epoch gate.
    #[test]
    fn w6_advance_epoch_via_sandbox_clock() {
        let (mut c, _) = fresh_contract(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(constants::EPOCH_DURATION_SEC / 3600);
        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.get_macro_state().epoch_ends_sec, c.macro_state.epoch_ends_sec);
    }

    // ------------------------------------------------------------------
    // Dividends — the u128 `inflow × 1e18` overflow in advance_epoch (a live bug)
    // ------------------------------------------------------------------

    #[test]
    fn div_mul_div_u128_exact_and_saturating() {
        use crate::mul_div_u128;
        assert_eq!(mul_div_u128(10, 20, 3), 66);
        assert_eq!(mul_div_u128(10u128.pow(30), 10u128.pow(18), 10u128.pow(12)), 10u128.pow(36));
        assert_eq!(mul_div_u128(u128::MAX, u128::MAX, u128::MAX), u128::MAX);
        assert_eq!(mul_div_u128(u128::MAX, 2, 4), u128::MAX / 2);
        assert_eq!(mul_div_u128(10u128.pow(30), 10u128.pow(18), 1), u128::MAX);
        assert_eq!(mul_div_u128(1, 1, 0), 0);
    }

    /// An inflow of 1e21..1e30 (live: 3.537e23) → a closed epoch + an end-to-end claim
    /// paying the whole inflow to the single contributor; V-04 solvent.
    #[test]
    fn div_large_inflow_closed_epoch_and_claim() {
        let (mut c, id) = fresh_contract(10u128.pow(24), StrainRole::Synthesizer, 0);
        for (epoch, exp) in [21u32, 23, 24, 27, 30].into_iter().enumerate() {
            let _ = c
                .inventories
                .insert((id, constants::ELIXIR_AETHER.to_string()), 3);
            ctx(OWNER, 0, T0);
            c.synthesize_and_redeem_aether(id, U128(2));
            // Simulate a large inflow (recorded as circulating supply → V-04 stays balanced).
            let inflow = 10u128.pow(exp);
            c.macro_state.total_snacc_supply += inflow;
            c.macro_state.epoch_dividend_inflow += inflow;
            c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
            c.advance_epoch(); // previously: an overflow panic
            let payout = c.claim_industrial_dividend(id, epoch as u64);
            assert_eq!(payout.0, inflow, "exp={exp}");
            assert!(c.get_global_solvency().solvent, "exp={exp}");
        }
    }

    // ------------------------------------------------------------------
    // W10 — lifecycle: splice BMR −15%, the dominant's status, MAX_GENERATION, Distressed
    // ------------------------------------------------------------------

    /// Two strains owned by OWNER (dom Synthesizer BMR 150, abs Extractor).
    fn two_strains() -> (SnaccContract, StrainId, StrainId) {
        let (mut c, dom) = fresh_contract(10_000, StrainRole::Synthesizer, 0);
        ctx(OWNER, 10_000, T0);
        let abs = c.incubate_strain(StrainRole::Extractor, U128(0), 0).strain_id;
        (c, dom, abs)
    }

    #[test]
    fn w10_splice_bmr_down_15_percent_and_vitality_settled() {
        let (mut c, dom, abs) = two_strains();
        ctx(OWNER, 0, T0 + 10 * 3600); // 10 h: dom eff = 8500 - 10×150 = 7000
        let v = c.splice_strains(dom, abs);
        assert_eq!(v.bmr_per_hour, 150 * 8500 / 10_000); // 127
        assert_eq!(v.vitality, 7000, "settled to now before the new BMR");
        assert_eq!(v.last_fed_sec, T0 + 10 * 3600);
        assert_eq!(v.effective_vitality, 7000);
        assert_eq!(v.generation, 1);
        // The new BMR applies going forward: +10 hours → 7000 - 10×127.
        ctx(OWNER, 0, T0 + 20 * 3600);
        assert_eq!(c.get_strain(dom).effective_vitality, 7000 - 10 * 127);
    }

    #[test]
    #[should_panic(expected = "ERR_STRAIN_ALREADY_SPLICED")]
    fn w10_splice_dominant_already_absorbed_rejected() {
        let (mut c, dom, abs) = two_strains();
        ctx(OWNER, 10_000, T0);
        let third = c.incubate_strain(StrainRole::Fermenter, U128(0), 0).strain_id;
        ctx(OWNER, 0, T0);
        let _ = c.splice_strains(dom, abs); // abs → SplicedBurned
        let _ = c.splice_strains(abs, third); // abs as dominant → rejected
    }

    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn w10_splice_dominant_tier3_rejected() {
        let (mut c, dom) = fresh_contract(10_000, StrainRole::Synthesizer, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(60); // dom eff = 8500 - 9000 → 0
        ctx(OWNER, 10_000, T0);
        let abs = c.incubate_strain(StrainRole::Extractor, U128(0), 0).strain_id;
        ctx(OWNER, 0, T0);
        let _ = c.splice_strains(dom, abs);
    }

    #[test]
    #[should_panic(expected = "ERR_MAX_GENERATION")]
    fn w10_splice_max_generation_rejected() {
        let (mut c, dom, abs) = two_strains();
        let mut v = c.strains.get(&dom).unwrap().clone();
        v.generation = constants::MAX_GENERATION;
        let _ = c.strains.insert(dom, v);
        ctx(OWNER, 0, T0);
        let _ = c.splice_strains(dom, abs);
    }

    #[test]
    #[should_panic(expected = "ERR_MAX_GENERATION")]
    fn w10_hostile_max_generation_rejected() {
        // A target owned by ANOTHER account ages into Tier 2; the raider OWNER is generation 8.
        ctx(OWNER, 0, T0);
        let mut c = SnaccContract::new();
        ctx(OTHER, 10_000, T0);
        let target = c.incubate_strain(StrainRole::Extractor, U128(0), 0).strain_id;
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(70); // target eff 1500
        ctx(OWNER, 10_000, T0);
        let raider = c.incubate_strain(StrainRole::Synthesizer, U128(0), 0).strain_id;
        let mut v = c.strains.get(&raider).unwrap().clone();
        v.generation = constants::MAX_GENERATION;
        let _ = c.strains.insert(raider, v);
        let nonce = c.get_strain(target).state_nonce;
        ctx(OWNER, 0, T0);
        let _ = c.hostile_takeover_splice(raider, target, nonce);
    }

    /// The `status` view & `get_solvency_tier` follow the I-02 ladder via Lazy BMR.
    #[test]
    fn w10_view_distressed_and_solvency_tier() {
        use crate::state::{SolvencyTier, StrainStatus};
        let (mut c, id) = fresh_contract(10_000, StrainRole::Extractor, 0);
        assert_eq!(c.get_strain(id).status, StrainStatus::Active);
        assert_eq!(c.get_solvency_tier(id), SolvencyTier::Tier1Healthy);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(65); // 8500 - 6500 = 2000 → Tier 2 (the boundary)
        assert_eq!(c.get_strain(id).status, StrainStatus::Distressed);
        assert_eq!(c.get_solvency_tier(id), SolvencyTier::Tier2Chapter11Distressed);
        let _ = c.advance_sandbox_clock(20); // → 0
        assert_eq!(c.get_solvency_tier(id), SolvencyTier::Tier3Chapter7Insolvent);
        ctx(OTHER, 0, T0);
        let _ = c.harvest_starving_strain(id);
        assert_eq!(c.get_strain(id).status, StrainStatus::Liquidated);
    }

    // ------------------------------------------------------------------
    // W8 — sub-account names unique across state generations
    // ------------------------------------------------------------------

    fn ctx_h(contract: &str, predecessor: &str, deposit: u128, height: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(contract))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .block_height(height)
            .block_timestamp(T0 * 1_000_000_000);
        testing_env!(b.build());
    }

    /// A state reset (migrate) → `next_strain_id` restarts at 1, but strain 1's
    /// sub-account name differs from the previous generation (no clash).
    #[test]
    fn w8_subaccount_unique_across_migrate() {
        const K: &str = "snacc-v001.testnet";
        ctx_h(K, OWNER, 0, 100);
        let mut c = SnaccContract::new();
        ctx_h(K, OWNER, 10_000, 100);
        let old_strain = c.incubate_strain(StrainRole::Extractor, U128(0), 0);
        assert_eq!(old_strain.vault_subaccount.to_string(), "s-1-100.snacc-v001.testnet");

        ctx_h(K, K, 0, 250);
        let mut m = SnaccContract::migrate();
        ctx_h(K, OWNER, 10_000, 251);
        let new_strain = m.incubate_strain(StrainRole::Extractor, U128(0), 0);
        assert_eq!(new_strain.strain_id, 1, "the id restarts after a state reset");
        assert_eq!(new_strain.vault_subaccount.to_string(), "s-1-250.snacc-v001.testnet");
        assert_ne!(old_strain.vault_subaccount, new_strain.vault_subaccount);
    }

    /// `materialize_subaccount` uses the name from the ledger (the same as the view), not
    /// a rebuilt legacy `s-<id>`.
    #[test]
    fn w8_materialize_uses_ledger_name() {
        const K: &str = "snacc-v001.testnet";
        ctx_h(K, OWNER, 0, 77);
        let mut c = SnaccContract::new();
        ctx_h(K, OWNER, 10_000, 77);
        let v = c.incubate_strain(StrainRole::Extractor, U128(0), 0);
        ctx_h(K, OWNER, 20_000_000_000_000_000_000_000, 78);
        let _ = c.materialize_subaccount(v.strain_id);
        assert_eq!(c.get_strain(v.strain_id).vault_subaccount.to_string(), "s-1-77.snacc-v001.testnet");
        assert_eq!(c.get_strain(v.strain_id).pending_callbacks, 1);
    }

    /// Pro-rata of two contributors (2:1) on a large inflow — exact without the 1e18 scale.
    #[test]
    fn div_pro_rata_two_contributors_large_inflow() {
        let (mut c, a) = fresh_contract(10u128.pow(24), StrainRole::Synthesizer, 0);
        ctx(OWNER, 10u128.pow(24), T0);
        let b = c.incubate_strain(StrainRole::Synthesizer, U128(0), 0).strain_id;
        for id in [a, b] {
            let _ = c.inventories.insert((id, constants::ELIXIR_AETHER.to_string()), 5);
        }
        ctx(OWNER, 0, T0);
        c.synthesize_and_redeem_aether(a, U128(2));
        c.synthesize_and_redeem_aether(b, U128(1));
        let inflow = 3 * 10u128.pow(29);
        c.macro_state.total_snacc_supply += inflow;
        c.macro_state.epoch_dividend_inflow += inflow;
        let _ = c.advance_sandbox_clock(24);
        c.advance_epoch();
        assert_eq!(c.claim_industrial_dividend(a, 0).0, 2 * 10u128.pow(29));
        assert_eq!(c.claim_industrial_dividend(b, 0).0, 10u128.pow(29));
        assert!(c.get_global_solvency().solvent);
    }
}

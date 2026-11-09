//! QA-101..105 (TC-F1-*) — Feature 1 test suite:
//! - QA-101: Hatch signless (`incubate_strain` via attached NEAR or internal `$SNACC` debit,
//!   single-role validation, 80/20 treasury split, supply conservation)
//! - QA-102: Sub-account (`materialize_subaccount` deposit >= 20 mN, async lock `pending_callbacks`,
//!   `state_nonce` mutation, callback resolution)
//! - QA-103: Cytokinesis (`divide_strain`: 50/50 balance & vitality, floor division for spores
//!   and organelles, Gen+1, ATP cost = 3, conservation invariants)
//! - QA-104: Colony views (`get_colony` filters living cells, excludes Liquidated and SplicedBurned,
//!   `get_colony_atp` aggregates active colony ATP budget)
//! - QA-105: Colony fences (`MAX_COLONY_SIZE=5`, `MAX_GENERATION=8`, and `EN-124` demographics gate:
//!   `total_divides <= total_mergers + total_lyses`).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod qa_f1_tests {
    use crate::state::constants;
    use crate::state::{OrganelleSlots, StrainRole, StrainStatus};
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    const ALKEMIS: &str = "alkemis.testnet";
    const INTRUDER: &str = "intruder.testnet";
    const CONTRACT_ID: &str = "snacc-v001.testnet";
    const T0: u64 = 1_000;

    fn acct(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }

    fn ctx_custom(contract: &str, predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(contract))
            .signer_account_id(acct(predecessor))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }

    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        ctx_custom(CONTRACT_ID, predecessor, deposit_yocto, ts_sec);
    }

    fn fresh_contract() -> SnaccContract {
        ctx(ALKEMIS, 0, T0);
        SnaccContract::new()
    }

    fn strain_snacc(c: &SnaccContract, id: u64) -> u128 {
        c.snacc_balances.get(&id).copied().unwrap_or(0)
    }

    // =========================================================================
    // QA-101 (TC-F1-01): Hatch Signless & Incubation
    // =========================================================================

    /// QA-101: Hatch with attached NEAR deposit (genesis onboarding).
    /// Supply is born from the NEAR stake: 20% to Treasury, 80% to Strain working capital.
    #[test]
    fn tc_f1_01_hatch_with_near_deposit_and_80_20_split() {
        let mut c = fresh_contract();
        let deposit_stake = 10 * 10u128.pow(24); // 10 NEAR
        let initial_spores = 100u64;

        ctx(ALKEMIS, deposit_stake, T0);
        let view = c.incubate_strain(StrainRole::Extractor, U128(deposit_stake), initial_spores);

        // 1. Strain identity & status
        assert_eq!(view.strain_id, 1, "First strain id must be 1");
        assert_eq!(view.owner_id, acct(ALKEMIS));
        assert_eq!(view.role_mask, constants::ROLE_EXTRACTOR);
        assert_eq!(view.vitality, constants::INITIAL_VITALITY);
        assert_eq!(view.effective_vitality, constants::INITIAL_VITALITY);
        assert_eq!(view.efficiency_bps, constants::INITIAL_EFFICIENCY_BPS);
        assert_eq!(view.atp_balance, constants::INITIAL_ATP);
        assert_eq!(view.generation, 0, "Genesis strain is gen 0");
        assert_eq!(view.state_nonce, 0);
        assert_eq!(view.pending_callbacks, 0);
        assert_eq!(view.status, StrainStatus::Active);

        // 2. 80/20 balance sheet split (F1 / D6 / D7)
        let expected_treasury = deposit_stake * 2_000 / 10_000;
        let expected_strain = deposit_stake - expected_treasury;
        assert_eq!(c.treasury, expected_treasury, "20% fee to Treasury");
        assert_eq!(strain_snacc(&c, 1), expected_strain, "80% working capital to Strain");
        assert_eq!(c.macro_state.total_snacc_supply, deposit_stake, "New supply minted from attached NEAR");

        // 3. Spores inventory (D3)
        assert_eq!(
            c.get_inventory(1, constants::GENESIS_SPORE_TOKEN_ID.to_string()).0,
            initial_spores as u128
        );

        // 4. Alchemist profile & demographics
        let profile = c.alchemists.get(&acct(ALKEMIS)).expect("Alchemist profile registered");
        assert_eq!(profile.strain_count, 1);
        assert_eq!(profile.joined_sec, T0);
        assert_eq!(c.macro_state.total_births, 1, "Incubation increments total_births");
    }

    /// QA-101: Hatch signless (attached NEAR = 0).
    /// Stake debited from caller's internal `$SNACC` balance (e.g. from dividends or lysis bounties).
    /// Supply MUST NOT grow (no free minting, I-01).
    #[test]
    fn tc_f1_01_hatch_signless_from_account_balance() {
        let mut c = fresh_contract();
        let stake = 50_000u128;

        // Player has accumulated internal balance
        c.set_account_balance(&acct(ALKEMIS), stake);
        let initial_supply = c.macro_state.total_snacc_supply;
        let initial_treasury = c.treasury;

        ctx(ALKEMIS, 0, T0);
        let view = c.incubate_strain(StrainRole::Fermenter, U128(stake), 25);

        assert_eq!(view.strain_id, 1);
        assert_eq!(view.role_mask, constants::ROLE_FERMENTER);
        assert_eq!(c.account_balance(&acct(ALKEMIS)), 0, "Account balance debited");

        // Supply invariant: total supply must remain unchanged
        assert_eq!(
            c.macro_state.total_snacc_supply, initial_supply,
            "Signless incubation must NOT mint new supply"
        );

        // 80/20 split from debited stake
        let expected_treasury = stake * 2_000 / 10_000;
        let expected_strain = stake - expected_treasury;
        assert_eq!(c.treasury - initial_treasury, expected_treasury);
        assert_eq!(strain_snacc(&c, 1), expected_strain);
        assert_eq!(
            c.get_inventory(1, constants::GENESIS_SPORE_TOKEN_ID.to_string()).0,
            25
        );
    }

    /// QA-101: Signless incubation with insufficient internal balance panics.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn tc_f1_01_hatch_signless_insufficient_balance_rejected() {
        let mut c = fresh_contract();
        c.set_account_balance(&acct(ALKEMIS), 2_000);

        ctx(ALKEMIS, 0, T0);
        c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
    }

    /// QA-101: Incubation with 0 attached NEAR and 0 stake argument panics.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn tc_f1_01_hatch_signless_zero_stake_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);
        c.incubate_strain(StrainRole::Extractor, U128(0), 0);
    }

    /// QA-101: Incubation with attached deposit differing from non-zero stake argument panics.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn tc_f1_01_hatch_deposit_stake_mismatch_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        c.incubate_strain(StrainRole::Extractor, U128(5_000), 0);
    }

    // =========================================================================
    // QA-102 (TC-F1-02): Sub-account Materialization
    // =========================================================================

    /// QA-102: `materialize_subaccount` creates async lock, bumps state nonce, and checks deposit.
    #[test]
    fn tc_f1_02_materialize_subaccount_locks_and_bumps_nonce() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        let id = v.strain_id;

        assert_eq!(c.get_strain(id).pending_callbacks, 0);
        assert_eq!(c.get_strain(id).state_nonce, 0);

        // Attach exactly MIN_MATERIALIZE_DEPOSIT_MILLINEAR (20 mN = 2 * 10^22 yoctoNEAR)
        let min_deposit = constants::MIN_MATERIALIZE_DEPOSIT_MILLINEAR * 10u128.pow(21);
        ctx(ALKEMIS, min_deposit, T0 + 10);
        let _promise = c.materialize_subaccount(id);

        let strain_after = c.get_strain(id);
        assert_eq!(strain_after.pending_callbacks, 1, "Async lock engaged");
        assert_eq!(strain_after.state_nonce, 1, "Mutation monotonically increments state_nonce");
        assert_eq!(strain_after.vault_subaccount.to_string(), "s-1-0.snacc-v001.testnet");
    }

    /// QA-102: `materialize_subaccount` with deposit below 20 mN panics.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn tc_f1_02_materialize_subaccount_below_20_millinear_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);

        let below_min = 19 * 10u128.pow(21); // 19 mN < 20 mN
        ctx(ALKEMIS, below_min, T0 + 10);
        let _ = c.materialize_subaccount(v.strain_id);
    }

    /// QA-102: `materialize_subaccount` called by non-owner panics.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn tc_f1_02_materialize_subaccount_non_owner_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);

        let deposit = 20 * 10u128.pow(21);
        ctx(INTRUDER, deposit, T0 + 10);
        let _ = c.materialize_subaccount(v.strain_id);
    }

    /// QA-102: `materialize_subaccount` for nonexistent strain panics.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn tc_f1_02_materialize_subaccount_nonexistent_rejected() {
        let mut c = fresh_contract();
        let deposit = 20 * 10u128.pow(21);
        ctx(ALKEMIS, deposit, T0);
        let _ = c.materialize_subaccount(999);
    }

    /// QA-102: Callback `on_subaccount_created` releases lock on success and on failure.
    #[test]
    fn tc_f1_02_on_subaccount_created_releases_lock_on_both_paths() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        let id = v.strain_id;

        let deposit = 20 * 10u128.pow(21);
        ctx(ALKEMIS, deposit, T0);
        let _ = c.materialize_subaccount(id);
        assert_eq!(c.get_strain(id).pending_callbacks, 1);

        // Path A: Promise succeeds -> callback called by contract
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(CONTRACT_ID))
            .predecessor_account_id(acct(CONTRACT_ID))
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp((T0 + 5) * 1_000_000_000);
        testing_env!(
            b.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![near_sdk::PromiseResult::Successful(vec![])]
        );
        c.on_subaccount_created(
            id,
            acct("s-1-0.snacc-v001.testnet"),
            acct(ALKEMIS),
            U128(deposit),
        );
        assert_eq!(c.get_strain(id).pending_callbacks, 0, "Lock released on success");
        assert_eq!(c.get_strain(id).state_nonce, 2, "Nonce bumped after callback mutation");

        // Path B: Another promise failure path
        ctx(ALKEMIS, deposit, T0 + 10);
        let _ = c.materialize_subaccount(id);
        assert_eq!(c.get_strain(id).pending_callbacks, 1);

        let mut b_fail = VMContextBuilder::new();
        b_fail.current_account_id(acct(CONTRACT_ID))
            .predecessor_account_id(acct(CONTRACT_ID))
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp((T0 + 15) * 1_000_000_000);
        testing_env!(
            b_fail.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![near_sdk::PromiseResult::Failed]
        );
        c.on_subaccount_created(
            id,
            acct("s-1-0.snacc-v001.testnet"),
            acct(ALKEMIS),
            U128(deposit),
        );
        assert_eq!(c.get_strain(id).pending_callbacks, 0, "Lock released even when promise fails");
    }

    // =========================================================================
    // QA-103 (TC-F1-03): divide_strain (Cytokinesis)
    // =========================================================================

    /// QA-103: `divide_strain` cytokinesis splits balance, vitality, spores, organelles,
    /// increments generation, debits 3 ATP, and strictly preserves state conservation.
    #[test]
    fn tc_f1_03_divide_strain_splits_all_state_and_preserves_conservation() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let p_init = c.incubate_strain(StrainRole::Extractor, U128(10_000), 10);
        let p_id = p_init.strain_id;

        // Set rich parent state
        let mut parent_vessel = c.strains.get(&p_id).unwrap().clone();
        parent_vessel.organel = OrganelleSlots {
            mitokondria: 4,
            nukleus: 2,
            lisosom: 2,
        };
        parent_vessel.phenotype_trait = Some("luminescent".to_string());
        parent_vessel.atp_balance = 50;
        let _ = c.strains.insert(p_id, parent_vessel);

        let parent_snacc_0 = strain_snacc(&c, p_id); // 8,000
        let parent_vitality_0 = c.get_strain(p_id).vitality; // 8,500
        let parent_spores_0 = c.get_inventory(p_id, constants::GENESIS_SPORE_TOKEN_ID.to_string()).0; // 10

        // Satisfy EN-124 gate: unlock 1 divide by recording 1 merger
        c.macro_state.total_mergers = 1;

        ctx(ALKEMIS, 0, T0);
        let (parent_view, child_view) = c.divide_strain(p_id);

        // 1. Parent state post-division
        assert_eq!(parent_view.strain_id, p_id);
        assert_eq!(parent_view.atp_balance, 50 - 3, "Parent pays 3 ATP for cytokinesis");
        assert_eq!(parent_view.state_nonce, 1, "Parent recorded mutation");
        assert_eq!(parent_view.generation, 0, "Parent generation unchanged");

        // 2. Child state
        let c_id = child_view.strain_id;
        assert_eq!(c_id, 2, "Child gets next_strain_id");
        assert_eq!(child_view.owner_id, acct(ALKEMIS));
        assert_eq!(child_view.generation, 1, "Child generation = parent.generation + 1");
        assert_eq!(child_view.role_mask, parent_view.role_mask, "Child inherits role_mask");
        assert_eq!(child_view.efficiency_bps, parent_view.efficiency_bps, "Child inherits efficiency_bps");
        assert_eq!(child_view.phenotype_trait, parent_view.phenotype_trait, "Child inherits phenotype_trait");
        assert_eq!(child_view.atp_balance, constants::INITIAL_ATP, "Child starts with INITIAL_ATP (50)");
        assert_eq!(child_view.status, StrainStatus::Active);

        // 3. Balance conservation: parent + child == parent_snacc_0
        let p_snacc_1 = strain_snacc(&c, p_id);
        let c_snacc_1 = strain_snacc(&c, c_id);
        assert_eq!(c_snacc_1, parent_snacc_0 / 2);
        assert_eq!(p_snacc_1 + c_snacc_1, parent_snacc_0, "Total $SNACC balance conserved");

        // 4. Vitality conservation: parent + child == parent_vitality_0
        let p_vitality_1 = c.get_strain(p_id).vitality;
        let c_vitality_1 = c.get_strain(c_id).vitality;
        assert_eq!(c_vitality_1, parent_vitality_0 / 2);
        assert_eq!(p_vitality_1 + c_vitality_1, parent_vitality_0, "Total vitality conserved");

        // 5. Spores inventory conservation: parent + child == parent_spores_0
        let p_spores_1 = c.get_inventory(p_id, constants::GENESIS_SPORE_TOKEN_ID.to_string()).0;
        let c_spores_1 = c.get_inventory(c_id, constants::GENESIS_SPORE_TOKEN_ID.to_string()).0;
        assert_eq!(c_spores_1, parent_spores_0 / 2);
        assert_eq!(p_spores_1 + c_spores_1, parent_spores_0, "Total spores conserved");

        // 6. Organelle conservation
        let p_org = c.get_strain(p_id).organel;
        let c_org = c.get_strain(c_id).organel;
        assert_eq!(c_org.mitokondria, 2);
        assert_eq!(c_org.nukleus, 1);
        assert_eq!(c_org.lisosom, 1);
        assert_eq!(p_org.mitokondria + c_org.mitokondria, 4);
        assert_eq!(p_org.nukleus + c_org.nukleus, 2);
        assert_eq!(p_org.lisosom + c_org.lisosom, 2);

        // 7. Demographics update
        assert_eq!(c.macro_state.total_divides, 1);
        assert_eq!(c.macro_state.total_births, 2, "Births = 1 incubate + 1 divide");
    }

    /// QA-103: `divide_strain` handles odd quantities with strict conservation (floor to child, parent keeps remainder).
    #[test]
    fn tc_f1_03_divide_strain_odd_values_conservation() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        let id = v.strain_id;

        // Set odd values
        c.set_strain_snacc(id, 10_001);
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.vitality = 8_501;
        vessel.organel = OrganelleSlots {
            mitokondria: 3,
            nukleus: 1,
            lisosom: 5,
        };
        let _ = c.strains.insert(id, vessel);
        let _ = c.inventories.insert((id, constants::GENESIS_SPORE_TOKEN_ID.to_string()), 7);

        c.macro_state.total_mergers = 1; // unlock divide
        ctx(ALKEMIS, 0, T0);
        let (_, child) = c.divide_strain(id);
        let c_id = child.strain_id;

        // Balance: 10_001 -> child 5_000, parent 5_001
        assert_eq!(strain_snacc(&c, c_id), 5_000);
        assert_eq!(strain_snacc(&c, id), 5_001);
        assert_eq!(strain_snacc(&c, id) + strain_snacc(&c, c_id), 10_001);

        // Vitality: 8_501 -> child 4_250, parent 4_251
        assert_eq!(c.get_strain(c_id).vitality, 4_250);
        assert_eq!(c.get_strain(id).vitality, 4_251);
        assert_eq!(c.get_strain(id).vitality + c.get_strain(c_id).vitality, 8_501);

        // Spores: 7 -> child 3, parent 4
        assert_eq!(c.get_inventory(c_id, constants::GENESIS_SPORE_TOKEN_ID.to_string()).0, 3);
        assert_eq!(c.get_inventory(id, constants::GENESIS_SPORE_TOKEN_ID.to_string()).0, 4);

        // Organelles
        let p_org = c.get_strain(id).organel;
        let c_org = c.get_strain(c_id).organel;
        assert_eq!(c_org.mitokondria, 1);
        assert_eq!(p_org.mitokondria, 2);
        assert_eq!(c_org.nukleus, 0);
        assert_eq!(p_org.nukleus, 1);
        assert_eq!(c_org.lisosom, 2);
        assert_eq!(p_org.lisosom, 3);
    }

    /// QA-103: `divide_strain` fails when parent has < 3 ATP.
    #[test]
    #[should_panic(expected = "ERR_NOT_ENOUGH_ATP")]
    fn tc_f1_03_divide_strain_insufficient_atp_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        let id = v.strain_id;

        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.atp_balance = 2; // Need 3
        let _ = c.strains.insert(id, vessel);

        c.macro_state.total_mergers = 1;
        ctx(ALKEMIS, 0, T0);
        c.divide_strain(id);
    }

    /// QA-103: `divide_strain` fails when caller is not owner.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn tc_f1_03_divide_strain_non_owner_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);

        c.macro_state.total_mergers = 1;
        ctx(INTRUDER, 0, T0);
        c.divide_strain(v.strain_id);
    }

    /// QA-103: `divide_strain` fails when parent is starving (`effective_vitality == 0`).
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn tc_f1_03_divide_strain_starving_parent_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        let id = v.strain_id;

        c.macro_state.total_mergers = 1;
        // Extractor BMR = 100/hour. Vitality 8,500 decays to 0 after 85 hours (85 * 3600 sec)
        ctx(ALKEMIS, 0, T0 + 90 * 3600);
        c.divide_strain(id);
    }

    /// QA-103: `divide_strain` fails on already spliced/burned parent.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_ALREADY_SPLICED")]
    fn tc_f1_03_divide_strain_already_spliced_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        let id = v.strain_id;

        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.status = StrainStatus::SplicedBurned;
        let _ = c.strains.insert(id, vessel);

        c.macro_state.total_mergers = 1;
        ctx(ALKEMIS, 0, T0);
        c.divide_strain(id);
    }

    // =========================================================================
    // QA-104 (TC-F1-04): get_colony and get_colony_atp
    // =========================================================================

    /// QA-104: `get_colony` returns only living cells, filtering out `SplicedBurned` & `Liquidated`.
    #[test]
    fn tc_f1_04_get_colony_filters_living_cells_and_excludes_dead() {
        let mut c = fresh_contract();

        // Incubate 3 strains
        ctx(ALKEMIS, 10_000, T0);
        let s1 = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
        ctx(ALKEMIS, 10_000, T0);
        let s2 = c.incubate_strain(StrainRole::Fermenter, U128(10_000), 0).strain_id;
        ctx(ALKEMIS, 10_000, T0);
        let s3 = c.incubate_strain(StrainRole::Synthesizer, U128(10_000), 0).strain_id;

        let colony_init = c.get_colony(acct(ALKEMIS));
        assert_eq!(colony_init.len(), 3, "All 3 living strains are in the colony");

        // 1. Splice s2 into s1 -> s2 becomes SplicedBurned
        ctx(ALKEMIS, 0, T0);
        let _ = c.splice_strains(s1, s2);
        assert_eq!(c.get_strain(s2).status, StrainStatus::SplicedBurned);

        let colony_after_splice = c.get_colony(acct(ALKEMIS));
        assert_eq!(colony_after_splice.len(), 2, "Spliced cell excluded from colony");
        assert!(colony_after_splice.iter().all(|s| s.strain_id != s2));

        // 2. Liquidate s3 after starvation -> s3 becomes Liquidated
        ctx(INTRUDER, 0, T0 + 90 * 3600); // 90h later, vitality = 0
        let _ = c.harvest_starving_strain(s3);
        assert_eq!(c.get_strain(s3).status, StrainStatus::Liquidated);

        let colony_final = c.get_colony(acct(ALKEMIS));
        assert_eq!(colony_final.len(), 1, "Only single living cell s1 remains");
        assert_eq!(colony_final[0].strain_id, s1);
        assert!(
            matches!(colony_final[0].status, StrainStatus::Active | StrainStatus::Distressed),
            "Living cell in colony retains active/distressed status"
        );
    }

    /// QA-104: `get_colony_atp` aggregates the ATP sum of active cells only.
    #[test]
    fn tc_f1_04_get_colony_atp_computes_active_colony_sum() {
        let mut c = fresh_contract();

        ctx(ALKEMIS, 10_000, T0);
        let s1 = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
        ctx(ALKEMIS, 10_000, T0);
        let s2 = c.incubate_strain(StrainRole::Fermenter, U128(10_000), 0).strain_id;

        // Set custom ATP values
        let mut v1 = c.strains.get(&s1).unwrap().clone();
        v1.atp_balance = 35;
        let _ = c.strains.insert(s1, v1);

        let mut v2 = c.strains.get(&s2).unwrap().clone();
        v2.atp_balance = 60;
        let _ = c.strains.insert(s2, v2);

        assert_eq!(c.get_colony_atp(acct(ALKEMIS)).0, 35 + 60);

        // Splice s2 into s1: s2 dies
        ctx(ALKEMIS, 0, T0);
        let v1_after = c.splice_strains(s1, s2);
        // Splice cost = 5 ATP on dominant: 35 - 5 = 30
        assert_eq!(v1_after.atp_balance, 30);
        assert_eq!(
            c.get_colony_atp(acct(ALKEMIS)).0,
            30,
            "Dead s2 ATP is not included in active colony sum"
        );
    }

    /// QA-104: `get_colony` and `get_colony_atp` return empty / 0 for account without strains.
    #[test]
    fn tc_f1_04_get_colony_empty_for_unknown_account() {
        let c = fresh_contract();
        assert_eq!(c.get_colony(acct("stranger.testnet")).len(), 0);
        assert_eq!(c.get_colony_atp(acct("stranger.testnet")).0, 0);
    }

    // =========================================================================
    // QA-105 (TC-F1-05): Colony Fences & Demographics Invariant (EN-124)
    // =========================================================================

    /// QA-105: `divide_strain` rejects when colony reaches MAX_COLONY_SIZE (5).
    #[test]
    #[should_panic(expected = "ERR_COLONY_FULL")]
    fn tc_f1_05_colony_full_fence_at_5() {
        let mut c = fresh_contract();

        // Incubate 5 strains (filling the colony to max size 5)
        for i in 0..5 {
            ctx(ALKEMIS, 10_000, T0 + i);
            c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        }
        assert_eq!(c.get_colony(acct(ALKEMIS)).len(), 5);

        // Grant divide quota
        c.macro_state.total_mergers = 10;

        ctx(ALKEMIS, 0, T0 + 10);
        // Attempting to divide when colony is already 5 must panic ERR_COLONY_FULL
        c.divide_strain(1);
    }

    /// QA-105: `divide_strain` rejects when child generation would exceed MAX_GENERATION (8).
    #[test]
    #[should_panic(expected = "ERR_MAX_GENERATION")]
    fn tc_f1_05_max_generation_fence_at_8() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let v = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
        let id = v.strain_id;

        // Set parent generation to 8 (MAX_GENERATION)
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.generation = constants::MAX_GENERATION;
        let _ = c.strains.insert(id, vessel);

        c.macro_state.total_mergers = 1;
        ctx(ALKEMIS, 0, T0);
        // Child generation would be 9 > 8 -> panic ERR_MAX_GENERATION
        c.divide_strain(id);
    }

    /// QA-105: Demographics Invariant (EN-124): division before any merger/lysis is rejected.
    #[test]
    #[should_panic(expected = "ERR_BIRTHS_EXCEEDED")]
    fn tc_f1_05_divide_before_merger_or_lysis_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let s1 = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;

        ctx(ALKEMIS, 0, T0);
        c.divide_strain(s1);
    }

    /// QA-105: Demographics Invariant (EN-124): division exceeding quota is rejected.
    #[test]
    #[should_panic(expected = "ERR_BIRTHS_EXCEEDED")]
    fn tc_f1_05_divide_exceeding_merger_quota_rejected() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 10_000, T0);
        let s1 = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
        ctx(ALKEMIS, 10_000, T0);
        let s2 = c.incubate_strain(StrainRole::Fermenter, U128(10_000), 0).strain_id;

        // 1 merger
        ctx(ALKEMIS, 0, T0);
        let _ = c.splice_strains(s1, s2);

        // 1st divide succeeds (total_divides 1 <= 1 + 0)
        let (_, c1) = c.divide_strain(s1);

        // 2nd divide exceeds quota (2 <= 1 is false) -> panic ERR_BIRTHS_EXCEEDED
        c.divide_strain(c1.strain_id);
    }

    /// QA-105: Demographics Invariant (EN-124):
    /// `total_divides <= total_mergers + total_lyses`.
    /// Sequence of 1 merger -> 1 divide -> 1 lysis -> 2nd divide proves both pathways.
    #[test]
    fn tc_f1_05_en124_demographics_gate_divides_following_merges_and_lyses() {
        let mut c = fresh_contract();

        ctx(ALKEMIS, 10_000, T0);
        let s1 = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
        ctx(ALKEMIS, 10_000, T0);
        let s2 = c.incubate_strain(StrainRole::Fermenter, U128(10_000), 0).strain_id;
        ctx(ALKEMIS, 10_000, T0);
        let s3 = c.incubate_strain(StrainRole::Synthesizer, U128(10_000), 0).strain_id;

        // Step 1: Perform 1 merger (splice s2 into s1)
        // Dominant generation becomes max(0, 0) + 1 = 1
        ctx(ALKEMIS, 0, T0);
        let _ = c.splice_strains(s1, s2);
        assert_eq!(c.macro_state.total_mergers, 1);
        assert_eq!(c.macro_state.total_lyses, 0);
        assert_eq!(c.macro_state.total_divides, 0);

        // Step 2: Now 1st divide succeeds! (total_divides 1 <= 1 + 0)
        // Child generation = s1.generation (1) + 1 = 2
        ctx(ALKEMIS, 0, T0);
        let (p1, c1) = c.divide_strain(s1);
        assert_eq!(p1.strain_id, s1);
        assert_eq!(c1.generation, 2);
        assert_eq!(c.macro_state.total_divides, 1);
        assert_eq!(
            c.macro_state.total_divides,
            c.macro_state.total_mergers + c.macro_state.total_lyses
        );

        // Step 3: Perform 1 lysis (harvest starving s3)
        ctx(INTRUDER, 0, T0 + 90 * 3600); // 90h later, vitality = 0
        let _ = c.harvest_starving_strain(s3);
        assert_eq!(c.macro_state.total_lyses, 1);
        // Now total_mergers (1) + total_lyses (1) = 2.

        // Step 4: 2nd divide now succeeds! (total_divides 2 <= 1 + 1)
        // Child c2 generation = c1.generation (2) + 1 = 3
        ctx(ALKEMIS, 0, T0 + 90 * 3600);
        // Refresh c1's vitality to prevent starving check failure
        let mut child_vessel = c.strains.get(&c1.strain_id).unwrap().clone();
        child_vessel.last_fed_sec = T0 + 90 * 3600;
        child_vessel.vitality = 5000;
        let _ = c.strains.insert(c1.strain_id, child_vessel);

        let (_, c2) = c.divide_strain(c1.strain_id);
        assert_eq!(c.macro_state.total_divides, 2);
        assert_eq!(
            c.macro_state.total_divides,
            c.macro_state.total_mergers + c.macro_state.total_lyses
        );
        assert_eq!(c2.generation, 3, "Second division child has generation 3");
    }
}

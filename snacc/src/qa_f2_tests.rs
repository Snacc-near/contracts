//! QA-106..110 (TC-F2-*) — Feature 2 test suite:
//! - QA-106: Spore extraction (`extract_spores`: 1 unit per call, role check `ROLE_EXTRACTOR`,
//!   validates `SPORE_TOKEN_IDS`, ATP cost = 1, attached catalyst fee deposit, inventory mint,
//!   volume legs K3, metabolic recovery, events emitted: `atp_spent` and `extracted`).
//! - QA-107: Spore transmutation (`transmute_spores_to_enzyme`: requires `ROLE_FERMENTER`,
//!   `spores_to_burn` must be even / multiple of 2, recipe priority: amalgam -> redactase -> synthase,
//!   diet matrix: fever (-20% for Extractor), hypothermia (-20% for Synthesizer),
//!   ATP cost = 2, nucleus bonus +10%/level, volume legs K2 & K3, events: `atp_spent` and `transmuted`).
//! - QA-108: Elixir synthesis (`synthesize_elixir`: requires `ROLE_SYNTHESIZER`, recipes:
//!   homunculus (2 amalgam + 1 synthase = batch 3), homeostasis (1 amalgam + 1 redactase = batch 2),
//!   aether (2 amalgam + 1 synthase = batch 3, requires TEE `phenotype_trait` attestation),
//!   ATP cost = 3, nucleus bonus +10%/level, nominal catalyst fee (no diet discount),
//!   volume legs K2 & K3, events: `atp_spent` and `synthesized`).
//! - QA-109: Lazy BMR & Metabolic Recovery (aging decay per caste: Extractor=100/h, Fermenter=120/h,
//!   Synthesizer=150/h; starvation check `effective_vitality == 0` panics `ERR_STRAIN_STARVING`;
//!   metabolic recovery: `min(10000, effective + 500)`, `last_fed_sec = now`).
//! - QA-110: ATP allocation & minting (`allocate_atp`: 1x per epoch, organel levels <= 5,
//!   cost = 1 ATP per level increase, updates organel and `last_allocate_epoch`;
//!   `calc_atp_minted`: formula base + mitochondria +10%/level; feeding asserts delta ATP == atp_minted).
#[cfg(test)]
#[allow(clippy::module_inception)]
mod qa_f2_tests {
    use crate::state::constants;
    use crate::state::{OrganelleSlots, StrainRole};
    use crate::SnaccContract;
    use crate::StrainId;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    const ALKEMIS: &str = "alkemis.testnet";
    const INTRUDER: &str = "intruder.testnet";
    const CONTRACT_ID: &str = "snacc-v001.testnet";
    const T0: u64 = 1_000;
    const NOMINAL_FEE: u128 = 10_000;

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
        role: StrainRole,
        deposit_stake: u128,
        initial_spores: u64,
    ) -> (SnaccContract, StrainId) {
        ctx(ALKEMIS, deposit_stake, T0);
        let mut c = SnaccContract::new();
        let view = c.incubate_strain(role, U128(deposit_stake), initial_spores);
        (c, view.strain_id)
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
    // QA-106 (TC-F2-01..05): Spore Extraction (`extract_spores`)
    // =========================================================================

    /// QA-106: Extraction succeeds for all three canonical spore types (ignis, cryo, lumin).
    /// Asserts 1 unit minted, 1 ATP spent, catalyst fee collected, and metabolism recovered.
    #[test]
    fn tc_f2_01_extract_spores_all_token_types_success() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        let initial_atp = c.get_strain(id).atp_balance;
        assert_eq!(initial_atp, constants::INITIAL_ATP); // 50

        // Extract 1: ignis
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let v1 = c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));
        assert_eq!(inv_balance(&c, id, constants::SPORE_TOKEN_IDS[0]), 1);
        assert_eq!(v1.atp_balance, initial_atp - 1);
        assert_eq!(v1.last_fed_sec, T0 + 10);
        assert_eq!(v1.state_nonce, 1);

        // Extract 2: cryo
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 20);
        let v2 = c.extract_spores(id, constants::SPORE_TOKEN_IDS[1].to_string(), U128(NOMINAL_FEE));
        assert_eq!(inv_balance(&c, id, constants::SPORE_TOKEN_IDS[1]), 1);
        assert_eq!(v2.atp_balance, initial_atp - 2);
        assert_eq!(v2.last_fed_sec, T0 + 20);
        assert_eq!(v2.state_nonce, 2);

        // Extract 3: lumin
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 30);
        let v3 = c.extract_spores(id, constants::SPORE_TOKEN_IDS[2].to_string(), U128(NOMINAL_FEE));
        assert_eq!(inv_balance(&c, id, constants::SPORE_TOKEN_IDS[2]), 1);
        assert_eq!(v3.atp_balance, initial_atp - 3);
        assert_eq!(v3.last_fed_sec, T0 + 30);
        assert_eq!(v3.state_nonce, 3);

        // Macro catalyst fee accumulator
        assert_eq!(c.macro_state.total_catalyst_fees, NOMINAL_FEE * 3);
    }

    /// QA-106: `extract_spores` requires ROLE_EXTRACTOR. Fermenter must be rejected.
    #[test]
    #[should_panic(expected = "ERR_ROLE_NOT_ALLOWED")]
    fn tc_f2_01_extract_requires_extractor_role() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));
    }

    /// QA-106: `extract_spores` called by non-owner must panic ERR_NOT_STRAIN_OWNER.
    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn tc_f2_01_extract_non_owner_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);
        ctx(INTRUDER, NOMINAL_FEE, T0 + 10);
        c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));
    }

    /// QA-106: `extract_spores` with unknown spore token must panic ERR_UNKNOWN_TOKEN.
    #[test]
    #[should_panic(expected = "ERR_UNKNOWN_TOKEN")]
    fn tc_f2_01_extract_unknown_token_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.extract_spores(id, "mt:spore-unknown".to_string(), U128(NOMINAL_FEE));
    }

    /// QA-106: `extract_spores` when strain has 0 ATP must panic ERR_NOT_ENOUGH_ATP.
    #[test]
    #[should_panic(expected = "ERR_NOT_ENOUGH_ATP")]
    fn tc_f2_01_extract_insufficient_atp_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        // Deplete ATP to 0
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.atp_balance = 0;
        let _ = c.strains.insert(id, vessel);

        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));
    }

    /// QA-106: `extract_spores` with deposit != catalyst fee must panic ERR_STAKE_DEPOSIT_MISMATCH.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn tc_f2_01_extract_deposit_mismatch_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);
        ctx(ALKEMIS, NOMINAL_FEE - 1, T0 + 10);
        c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));
    }

    /// QA-106: `extract_spores` updates K3 volume legs (0 burn, 1 mint).
    #[test]
    fn tc_f2_01_extract_volume_legs_k3_tracked() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        let v_epoch_before = c.macro_state.volume_epoch;
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let _ = c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));

        // Volume legs formula: (burn(0) + mint(1)) * VOLUME_SCALE(5) * P_BASE(100) = 500
        let expected_legs = constants::VOLUME_SCALE * constants::P_BASE;
        assert_eq!(
            c.macro_state.volume_epoch,
            v_epoch_before + NOMINAL_FEE + expected_legs
        );
    }

    // =========================================================================
    // QA-107 (TC-F2-06..12): Spore Transmutation (`transmute_spores_to_enzyme`)
    // =========================================================================

    /// QA-107: Recipe 1 — Amalgam (2x ignis per batch).
    /// Burns 4 ignis, mints 4 amalgam at 10000 bps efficiency, consumes 2 ATP.
    #[test]
    fn tc_f2_02_transmute_amalgam_recipe_two_ignis() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 6); // 6 ignis

        let initial_atp = c.get_strain(id).atp_balance;
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let v = c.transmute_spores_to_enzyme(id, 4, U128(NOMINAL_FEE));

        assert_eq!(inv_balance(&c, id, constants::SPORE_TOKEN_IDS[0]), 2);
        assert_eq!(inv_balance(&c, id, constants::ENZYME_AMALGAM), 4);
        assert_eq!(v.atp_balance, initial_atp - 2);
        assert_eq!(v.last_fed_sec, T0 + 10);
        assert_eq!(v.state_nonce, 1);
    }

    /// QA-107: Recipe 2 — Redactase (1x ignis + 1x lumin).
    /// Triggered when ignis is insufficient for pure amalgam, but k ignis and k lumin exist.
    #[test]
    fn tc_f2_02_transmute_redactase_recipe_ignis_plus_lumin() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        // Need 4 spores burned -> k = 2. Provide 2 ignis and 3 lumin.
        // ignis (2) < burn (4) -> fails Amalgam.
        // ignis (2) >= 2 && lumin (3) >= 2 -> matches Redactase.
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 2);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[2], 3);

        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let v = c.transmute_spores_to_enzyme(id, 4, U128(NOMINAL_FEE));

        assert_eq!(inv_balance(&c, id, constants::SPORE_TOKEN_IDS[0]), 0);
        assert_eq!(inv_balance(&c, id, constants::SPORE_TOKEN_IDS[2]), 1);
        assert_eq!(inv_balance(&c, id, constants::ENZYME_REDACTASE), 4);
        assert_eq!(v.atp_balance, constants::INITIAL_ATP - 2);
    }

    /// QA-107: Recipe 3 — Synthase (2x cryo per batch).
    /// Triggered when ignis is 0 and cryo >= burn.
    #[test]
    fn tc_f2_02_transmute_synthase_recipe_two_cryo() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[1], 4); // 4 cryo

        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let v = c.transmute_spores_to_enzyme(id, 4, U128(NOMINAL_FEE));

        assert_eq!(inv_balance(&c, id, constants::SPORE_TOKEN_IDS[1]), 0);
        assert_eq!(inv_balance(&c, id, constants::ENZYME_SYNTHASE), 4);
        assert_eq!(v.atp_balance, constants::INITIAL_ATP - 2);
    }

    /// QA-107: Transmute with odd number of spores must panic ERR_RECIPE_NOT_MET.
    #[test]
    #[should_panic(expected = "ERR_RECIPE_NOT_MET")]
    fn tc_f2_02_transmute_odd_spores_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 5);
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.transmute_spores_to_enzyme(id, 3, U128(NOMINAL_FEE));
    }

    /// QA-107: Transmute with zero spores must panic ERR_RECIPE_NOT_MET.
    #[test]
    #[should_panic(expected = "ERR_RECIPE_NOT_MET")]
    fn tc_f2_02_transmute_zero_spores_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.transmute_spores_to_enzyme(id, 0, U128(NOMINAL_FEE));
    }

    /// QA-107: Transmute called by non-Fermenter must panic ERR_ROLE_NOT_ALLOWED.
    #[test]
    #[should_panic(expected = "ERR_ROLE_NOT_ALLOWED")]
    fn tc_f2_02_transmute_requires_fermenter_role() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 4);
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.transmute_spores_to_enzyme(id, 2, U128(NOMINAL_FEE));
    }

    /// QA-107: Transmute when inventory is insufficient for any recipe panics ERR_INSUFFICIENT_BALANCE.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn tc_f2_02_transmute_insufficient_inventory_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        // Only 1 ignis available, but need at least 2
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 1);
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.transmute_spores_to_enzyme(id, 2, U128(NOMINAL_FEE));
    }

    /// QA-107: Nucleus organelle bonus increases transmute enzyme output by +10%/level.
    #[test]
    fn tc_f2_02_transmute_nucleus_bonus_output() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 20);

        // Set nucleus level to 2 (+20% output)
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.organel = OrganelleSlots {
            mitokondria: 0,
            nukleus: 2,
            lisosom: 0,
        };
        let _ = c.strains.insert(id, vessel);

        // Burn 10 ignis -> base minted = 10. With +20%: 10 * 12000 / 10000 = 12.
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let _ = c.transmute_spores_to_enzyme(id, 10, U128(NOMINAL_FEE));
        assert_eq!(inv_balance(&c, id, constants::ENZYME_AMALGAM), 12);
    }

    /// QA-107: Diet matrix in Hypothermia (<600 mK) for Synthesizer role:
    /// 20% discount on catalyst fee (`DIET_FAMINE_SYNTHESIZER_BPS = 2000`).
    /// Attached deposit must equal `fee * 0.8`.
    #[test]
    fn tc_f2_02_transmute_diet_matrix_discount_hypothermia_synth() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 4);

        // Grant Synthesizer role bit via splice / manual donor mask
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.role_mask |= constants::ROLE_SYNTHESIZER;
        let _ = c.strains.insert(id, vessel);

        // Ensure crucible temp is in Hypothermia (<600 mK)
        c.macro_state.volume_epoch = 0; // Temp = 0 mK < 600 mK

        let nominal_fee = 10_000u128;
        let discounted_fee = nominal_fee * 8_000 / 10_000; // 8_000

        // Transmute with fee=10000 attached deposit=8000
        ctx(ALKEMIS, discounted_fee, T0 + 10);
        let v = c.transmute_spores_to_enzyme(id, 2, U128(nominal_fee));

        assert_eq!(inv_balance(&c, id, constants::ENZYME_AMALGAM), 2);
        assert_eq!(v.atp_balance, constants::INITIAL_ATP - 2);
    }

    /// QA-107: Diet matrix in Fever (>610 mK) for Extractor role:
    /// 20% discount on catalyst fee (`DIET_FEVER_EXTRACTOR_BPS = 2000`).
    #[test]
    fn tc_f2_02_transmute_diet_matrix_discount_fever_extractor() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 4);

        // Grant Extractor role bit
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.role_mask |= constants::ROLE_EXTRACTOR;
        let _ = c.strains.insert(id, vessel);

        // Force temperature into Fever (>610 mK)
        // temp = 1000 * volume_epoch / total_snacc_supply
        c.macro_state.volume_epoch = c.macro_state.total_snacc_supply; // temp = 1000 mK > 610 mK
        c.macro_state.last_update_ms = (T0 + 10) * 1_000;

        let nominal_fee = 10_000u128;
        let discounted_fee = nominal_fee * 8_000 / 10_000; // 8_000

        ctx(ALKEMIS, discounted_fee, T0 + 10);
        let v = c.transmute_spores_to_enzyme(id, 2, U128(nominal_fee));
        assert_eq!(inv_balance(&c, id, constants::ENZYME_AMALGAM), 2);
        assert_eq!(v.atp_balance, constants::INITIAL_ATP - 2);
    }

    // =========================================================================
    // QA-108 (TC-F2-13..18): Elixir Synthesis (`synthesize_elixir`)
    // =========================================================================

    /// QA-108: Synthesis Recipe 1 — Homunculus (2 amalgam + 1 synthase = batch 3).
    /// Burns 6 enzymes (4 amalgam + 2 synthase), mints 2 elixirs, consumes 3 ATP.
    #[test]
    fn tc_f2_03_synthesize_homunculus_recipe_success() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 4);
        set_inv(&mut c, id, constants::ENZYME_SYNTHASE, 2);

        let initial_atp = c.get_strain(id).atp_balance;
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let v = c.synthesize_elixir(
            id,
            constants::ELIXIR_HOMUNCULUS.to_string(),
            6,
            U128(NOMINAL_FEE),
        );

        assert_eq!(inv_balance(&c, id, constants::ENZYME_AMALGAM), 0);
        assert_eq!(inv_balance(&c, id, constants::ENZYME_SYNTHASE), 0);
        assert_eq!(inv_balance(&c, id, constants::ELIXIR_HOMUNCULUS), 2);
        assert_eq!(v.atp_balance, initial_atp - 3);
        assert_eq!(v.last_fed_sec, T0 + 10);
        assert_eq!(v.state_nonce, 1);
    }

    /// QA-108: Synthesis Recipe 2 — Homeostasis (1 amalgam + 1 redactase = batch 2).
    /// Burns 4 enzymes (2 amalgam + 2 redactase), mints 2 elixirs, consumes 3 ATP.
    #[test]
    fn tc_f2_03_synthesize_homeostasis_recipe_success() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 2);
        set_inv(&mut c, id, constants::ENZYME_REDACTASE, 2);

        let initial_atp = c.get_strain(id).atp_balance;
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let v = c.synthesize_elixir(
            id,
            constants::ELIXIR_HOMEOSTASIS.to_string(),
            4,
            U128(NOMINAL_FEE),
        );

        assert_eq!(inv_balance(&c, id, constants::ENZYME_AMALGAM), 0);
        assert_eq!(inv_balance(&c, id, constants::ENZYME_REDACTASE), 0);
        assert_eq!(inv_balance(&c, id, constants::ELIXIR_HOMEOSTASIS), 2);
        assert_eq!(v.atp_balance, initial_atp - 3);
    }

    /// QA-108: Synthesis Recipe 3 — Aether requires TEE mutation attestation (`phenotype_trait.is_some()`).
    /// Without phenotype trait, synthesis must panic ERR_REQUIRES_TEE_ATTESTATION.
    #[test]
    #[should_panic(expected = "ERR_REQUIRES_TEE_ATTESTATION")]
    fn tc_f2_03_synthesize_aether_requires_tee_attestation() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 2);
        set_inv(&mut c, id, constants::ENZYME_SYNTHASE, 1);

        // phenotype_trait is None
        assert!(c.get_strain(id).phenotype_trait.is_none());

        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.synthesize_elixir(id, constants::ELIXIR_AETHER.to_string(), 3, U128(NOMINAL_FEE));
    }

    /// QA-108: Synthesis Recipe 3 — Aether succeeds when TEE mutation is present.
    #[test]
    fn tc_f2_03_synthesize_aether_with_tee_attestation_success() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 2);
        set_inv(&mut c, id, constants::ENZYME_SYNTHASE, 1);

        // Attach phenotype trait to simulate successful TEE mutation
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.phenotype_trait = Some("bioluminescent_crystallite".to_string());
        let _ = c.strains.insert(id, vessel);

        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let v = c.synthesize_elixir(id, constants::ELIXIR_AETHER.to_string(), 3, U128(NOMINAL_FEE));

        assert_eq!(inv_balance(&c, id, constants::ENZYME_AMALGAM), 0);
        assert_eq!(inv_balance(&c, id, constants::ENZYME_SYNTHASE), 0);
        assert_eq!(inv_balance(&c, id, constants::ELIXIR_AETHER), 1);
        assert_eq!(v.atp_balance, constants::INITIAL_ATP - 3);
    }

    /// QA-108: Synthesis with non-multiple batch size must panic ERR_RECIPE_NOT_MET.
    #[test]
    #[should_panic(expected = "ERR_RECIPE_NOT_MET")]
    fn tc_f2_03_synthesize_non_multiple_batch_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 4);
        set_inv(&mut c, id, constants::ENZYME_SYNTHASE, 2);

        // Homunculus requires batch size 3. Burning 4 is invalid.
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.synthesize_elixir(id, constants::ELIXIR_HOMUNCULUS.to_string(), 4, U128(NOMINAL_FEE));
    }

    /// QA-108: Synthesis called by non-Synthesizer must panic ERR_ROLE_NOT_ALLOWED.
    #[test]
    #[should_panic(expected = "ERR_ROLE_NOT_ALLOWED")]
    fn tc_f2_03_synthesize_requires_synthesizer_role() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 2);
        set_inv(&mut c, id, constants::ENZYME_REDACTASE, 2);

        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.synthesize_elixir(id, constants::ELIXIR_HOMEOSTASIS.to_string(), 2, U128(NOMINAL_FEE));
    }

    /// QA-108: Synthesis with insufficient enzyme stock must panic ERR_INSUFFICIENT_BALANCE.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn tc_f2_03_synthesize_insufficient_enzymes_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 1); // Need 2 amalgam
        set_inv(&mut c, id, constants::ENZYME_SYNTHASE, 1);

        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        c.synthesize_elixir(id, constants::ELIXIR_HOMUNCULUS.to_string(), 3, U128(NOMINAL_FEE));
    }

    /// QA-108: Nucleus organelle bonus increases elixir output by +10%/level.
    #[test]
    fn tc_f2_03_synthesize_nucleus_bonus_output() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        // Provide enzymes for 10 batches of homeostasis (10 amalgam + 10 redactase = 20 enzymes)
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 10);
        set_inv(&mut c, id, constants::ENZYME_REDACTASE, 10);

        // Set nucleus level to 3 (+30% output)
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.organel = OrganelleSlots {
            mitokondria: 0,
            nukleus: 3,
            lisosom: 0,
        };
        let _ = c.strains.insert(id, vessel);

        // 10 batches * 13000 / 10000 = 13 elixirs
        ctx(ALKEMIS, NOMINAL_FEE, T0 + 10);
        let _ = c.synthesize_elixir(
            id,
            constants::ELIXIR_HOMEOSTASIS.to_string(),
            20,
            U128(NOMINAL_FEE),
        );
        assert_eq!(inv_balance(&c, id, constants::ELIXIR_HOMEOSTASIS), 13);
    }

    /// QA-108: Elixir synthesis takes nominal fee without diet discounts (full nominal fee).
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn tc_f2_03_synthesize_nominal_fee_no_diet_discount() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 2);
        set_inv(&mut c, id, constants::ENZYME_REDACTASE, 2);

        // In Hypothermia, Synthesizer gets 20% discount on transmute, but NOT synthesize!
        c.macro_state.volume_epoch = 0; // Hypothermia
        let nominal_fee = 10_000u128;
        let discounted_deposit = 8_000u128;

        // Attaching discounted deposit must panic ERR_STAKE_DEPOSIT_MISMATCH
        ctx(ALKEMIS, discounted_deposit, T0 + 10);
        c.synthesize_elixir(
            id,
            constants::ELIXIR_HOMEOSTASIS.to_string(),
            2,
            U128(nominal_fee),
        );
    }

    // =========================================================================
    // QA-109 (TC-F2-19..23): Lazy BMR & Metabolic Recovery
    // =========================================================================

    /// QA-109: Lazy BMR decay rates per caste (§13a):
    /// Extractor = 100/h, Fermenter = 120/h, Synthesizer = 150/h.
    #[test]
    fn tc_f2_04_lazy_bmr_decay_rates_per_caste() {
        let (c_ext, id_ext) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);
        let (c_fer, id_fer) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        let (c_syn, id_syn) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);

        // After 10 hours (36,000 s):
        let t10 = T0 + 36_000;
        let v_ext = c_ext.get_strain(id_ext);
        let v_fer = c_fer.get_strain(id_fer);
        let v_syn = c_syn.get_strain(id_syn);

        // Extractor: 8500 - (10 * 100) = 7500
        let vessel_ext = c_ext.strains.get(&id_ext).unwrap();
        assert_eq!(vessel_ext.effective_vitality(t10), 7500);
        assert_eq!(v_ext.bmr_per_hour, 100);

        // Fermenter: 8500 - (10 * 120) = 7300
        let vessel_fer = c_fer.strains.get(&id_fer).unwrap();
        assert_eq!(vessel_fer.effective_vitality(t10), 7300);
        assert_eq!(v_fer.bmr_per_hour, 120);

        // Synthesizer: 8500 - (10 * 150) = 7000
        let vessel_syn = c_syn.strains.get(&id_syn).unwrap();
        assert_eq!(vessel_syn.effective_vitality(t10), 7000);
        assert_eq!(v_syn.bmr_per_hour, 150);
    }

    /// QA-109: Starvation gate blocks all F2 operations when effective vitality drops to 0.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn tc_f2_04_starving_gate_blocks_extraction() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        // Advance 90 hours: 90 * 100 = 9000 > 8500 -> effective vitality = 0
        let starving_ts = T0 + 90 * 3600;
        ctx(ALKEMIS, NOMINAL_FEE, starving_ts);
        c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));
    }

    /// QA-109: Starvation gate blocks transmutation when effective vitality drops to 0.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn tc_f2_04_starving_gate_blocks_transmutation() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Fermenter, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::SPORE_TOKEN_IDS[0], 4);

        // 80 hours: 80 * 120 = 9600 > 8500 -> effective vitality = 0
        let starving_ts = T0 + 80 * 3600;
        ctx(ALKEMIS, NOMINAL_FEE, starving_ts);
        c.transmute_spores_to_enzyme(id, 2, U128(NOMINAL_FEE));
    }

    /// QA-109: Starvation gate blocks elixir synthesis when effective vitality drops to 0.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn tc_f2_04_starving_gate_blocks_synthesis() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Synthesizer, 10 * 10u128.pow(24), 0);
        set_inv(&mut c, id, constants::ENZYME_AMALGAM, 2);
        set_inv(&mut c, id, constants::ENZYME_REDACTASE, 2);

        // 60 hours: 60 * 150 = 9000 > 8500 -> effective vitality = 0
        let starving_ts = T0 + 60 * 3600;
        ctx(ALKEMIS, NOMINAL_FEE, starving_ts);
        c.synthesize_elixir(id, constants::ELIXIR_HOMEOSTASIS.to_string(), 2, U128(NOMINAL_FEE));
    }

    /// QA-109: Metabolic recovery on F2 action: `vitality = min(10000, effective + 500)`, `last_fed_sec = now`.
    #[test]
    fn tc_f2_04_metabolic_recovery_effective_plus_500_capped() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        // Case A: 5 hours elapse -> decay = 500. Effective vitality = 8500 - 500 = 8000.
        let t1 = T0 + 5 * 3600;
        ctx(ALKEMIS, NOMINAL_FEE, t1);
        let v1 = c.extract_spores(id, constants::SPORE_TOKEN_IDS[0].to_string(), U128(NOMINAL_FEE));
        // Recovers 8000 + 500 = 8500
        assert_eq!(v1.vitality, 8500);
        assert_eq!(v1.last_fed_sec, t1);

        // Case B: Near cap recovery (vitality 9800 -> recovers to 10000, not 10300)
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.vitality = 9800;
        vessel.last_fed_sec = t1;
        let _ = c.strains.insert(id, vessel);

        ctx(ALKEMIS, NOMINAL_FEE, t1 + 10);
        let v2 = c.extract_spores(id, constants::SPORE_TOKEN_IDS[1].to_string(), U128(NOMINAL_FEE));
        assert_eq!(v2.vitality, constants::VITALITY_CAP); // 10000
    }

    // =========================================================================
    // QA-110 (TC-F2-24..28): `allocate_atp` & ATP Minting
    // =========================================================================

    /// QA-110: `calc_atp_minted` formula correctness:
    /// `atp_minted = (fee / ATP_YOCTO_UNIT) * efficiency/10000 * (10000 + 1000 * mito)/10000`.
    #[test]
    fn tc_f2_05_calc_atp_minted_formula_and_mitochondria_levels() {
        let unit = constants::ATP_YOCTO_UNIT; // 10^15 yocto

        // Base fee = 10 units, efficiency = 10000 bps (100%)
        // Level 0: 10 * 1.0 * 1.0 = 10
        assert_eq!(SnaccContract::calc_atp_minted(10 * unit, 10_000, 0), 10);
        // Level 1 (+10%): 10 * 1.1 = 11
        assert_eq!(SnaccContract::calc_atp_minted(10 * unit, 10_000, 1), 11);
        // Level 2 (+20%): 10 * 1.2 = 12
        assert_eq!(SnaccContract::calc_atp_minted(10 * unit, 10_000, 2), 12);
        // Level 5 (+50%): 10 * 1.5 = 15
        assert_eq!(SnaccContract::calc_atp_minted(10 * unit, 10_000, 5), 15);

        // Efficiency scaling: 8000 bps (80%)
        // Base 10 -> 8 at level 0; 8 * 1.5 = 12 at level 5
        assert_eq!(SnaccContract::calc_atp_minted(10 * unit, 8_000, 0), 8);
        assert_eq!(SnaccContract::calc_atp_minted(10 * unit, 8_000, 5), 12);

        // Fee < ATP_YOCTO_UNIT yields 0
        assert_eq!(SnaccContract::calc_atp_minted(unit - 1, 10_000, 5), 0);
    }

    /// QA-110: Feeding delta ATP strictly matches `atp_minted` (Audit A-09 strengthened assertion).
    #[test]
    fn tc_f2_05_feeding_delta_atp_matches_atp_minted() {
        let stake = 10 * 10u128.pow(24);
        let (mut c, id) = fresh_contract_with_strain(StrainRole::Extractor, stake, 0);

        // Disable dormancy discount and ensure full vitality for 100% rescue multiplier
        c.macro_state.params.dormancy_threshold_bps = 0;
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.vitality = constants::VITALITY_CAP;
        vessel.organel.mitokondria = 2;
        let _ = c.strains.insert(id, vessel);

        let feed_fee_yocto = 20 * constants::ATP_YOCTO_UNIT;
        let expected_atp_minted = SnaccContract::calc_atp_minted(
            feed_fee_yocto,
            constants::INITIAL_EFFICIENCY_BPS,
            2,
        );
        assert_eq!(expected_atp_minted, 24); // 20 * 1.2 = 24

        let atp_before = c.get_strain(id).atp_balance;
        ctx(ALKEMIS, 0, T0 + 10);
        let after_view = c.feed_strain(id, U128(feed_fee_yocto));
        let atp_after = after_view.atp_balance;

        // Delta ATP == atp_minted
        assert_eq!(
            atp_after - atp_before,
            expected_atp_minted,
            "Delta ATP must match calc_atp_minted verbatim"
        );
    }

    /// QA-110: `allocate_atp` updates organelles, charges 1 ATP per level increase, and advances state nonce.
    #[test]
    fn tc_f2_05_allocate_atp_costs_and_mutation() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        let atp_before = c.get_strain(id).atp_balance; // 50
        assert_eq!(atp_before, 50);

        // Allocate: mitokondria 2, nukleus 1, lisosom 1 -> total increase = 2 + 1 + 1 = 4 levels.
        ctx(ALKEMIS, 0, T0);
        let view = c.allocate_atp(id, 2, 1, 1);

        assert_eq!(view.organel.mitokondria, 2);
        assert_eq!(view.organel.nukleus, 1);
        assert_eq!(view.organel.lisosom, 1);
        assert_eq!(view.atp_balance, 50 - 4); // 46
        assert_eq!(view.last_allocate_epoch, c.macro_state.current_epoch);
        assert_eq!(view.state_nonce, 1);
    }

    /// QA-110: `allocate_atp` more than once in the same epoch must panic ERR_ALREADY_ALLOCATED.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_ALLOCATED")]
    fn tc_f2_05_allocate_atp_once_per_epoch_fence() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        ctx(ALKEMIS, 0, T0);
        let _ = c.allocate_atp(id, 1, 0, 0);

        // Second allocation in same epoch -> panic ERR_ALREADY_ALLOCATED
        c.allocate_atp(id, 2, 0, 0);
    }

    /// QA-110: `allocate_atp` exceeding MAX_ORGANEL_LEVEL (5) must panic ERR_ORGANEL_LEVEL_MAX.
    #[test]
    #[should_panic(expected = "ERR_ORGANEL_LEVEL_MAX")]
    fn tc_f2_05_allocate_atp_max_level_5_fence() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        ctx(ALKEMIS, 0, T0);
        c.allocate_atp(id, 6, 0, 0);
    }

    /// QA-110: `allocate_atp` with insufficient ATP balance must panic ERR_NOT_ENOUGH_ATP.
    #[test]
    #[should_panic(expected = "ERR_NOT_ENOUGH_ATP")]
    fn tc_f2_05_allocate_atp_insufficient_atp_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        // Set strain ATP to 1
        let mut vessel = c.strains.get(&id).unwrap().clone();
        vessel.atp_balance = 1;
        let _ = c.strains.insert(id, vessel);

        // Attempting to upgrade 2 levels (cost 2) > 1 ATP -> panic ERR_NOT_ENOUGH_ATP
        ctx(ALKEMIS, 0, T0);
        c.allocate_atp(id, 2, 0, 0);
    }

    /// QA-110: `allocate_atp` by non-owner must panic ERR_NOT_OWNED_BY_CALLEE.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn tc_f2_05_allocate_atp_non_owner_rejected() {
        let (mut c, id) =
            fresh_contract_with_strain(StrainRole::Extractor, 10 * 10u128.pow(24), 0);

        ctx(INTRUDER, 0, T0);
        c.allocate_atp(id, 1, 0, 0);
    }
}

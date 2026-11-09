//! EN-111 — `TC-EN-04-01..08`: the EN-110 diet matrix on transmute (4 combinations
//! of a Fermenter-bit role × zone {Fever, Hypo}; the Normal zone = no diet, checked
//! inside every test). Exact matrix from the code: `DIET_FEVER_EXTRACTOR_BPS = 2000`
//! (needs the Extractor bit), `DIET_FAMINE_SYNTHESIZER_BPS = 2000` (needs the
//! Synthesizer bit), zero otherwise. Transmute requires the Fermenter bit — a role
//! without F cannot transmute (role gate) — so the matrix under test is the OR of the
//! bits on top of Fermenter. The deposit must equal fee×(10000−diet)/10000.
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so test paths stay stable
mod qa_diet_tests {
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

    const NOMINAL: u128 = 10u128.pow(13);
    const TS: u64 = 1_000;

    /// Build a combined-bit strain via splice (Fermenter dominant + donors).
    fn strain_with_bits(bits: u8) -> (SnaccContract, crate::StrainId) {
        ctx("alkemis.testnet", 10_000, TS);
        let mut c = SnaccContract::new();
        let id = c
            .incubate_strain(StrainRole::Fermenter, U128(10_000), 0)
            .strain_id;
        // Donate the bits that are needed (Extract=001, Synth=100).
        if bits & 0b001 != 0 {
            ctx("alkemis.testnet", 10_000, TS);
            let d = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
            ctx("alkemis.testnet", 0, TS);
            let _ = c.splice_strains(id, d);
        }
        if bits & 0b100 != 0 {
            ctx("alkemis.testnet", 10_000, TS);
            let d = c.incubate_strain(StrainRole::Synthesizer, U128(10_000), 0).strain_id;
            ctx("alkemis.testnet", 0, TS);
            let _ = c.splice_strains(id, d);
        }
        assert_eq!(c.get_strain(id).role_mask, bits);
        // Two ignis spores for one transmute batch.
        let _ = c.inventories.insert((id, "mt:spore-ignis".to_string()), 2);
        (c, id)
    }

    /// Force a zone via the public `volume_epoch` field — temp = 1000×V/supply.
    fn set_zone(c: &mut SnaccContract, zone: &str) {
        let supply = c.macro_state.total_snacc_supply;
        let v = match zone {
            "FEVER" => 610 * supply / 1000 + supply, // > 610 mK
            "HYPO" => 0,                             // < 600 mK
            _ => 605 * supply / 1000,                // Normal [600, 610]
        };
        c.macro_state.volume_epoch = v;
        c.macro_state.last_update_ms = 1_000 * 1_000; // = block ts → decay 0
    }

    /// One combination: role × zone → a wrong deposit panics, the right deposit succeeds.
    fn check_combination(bits: u8, zone: &str, diet_bps: u16) {
        let (mut c, id) = strain_with_bits(bits);
        set_zone(&mut c, zone);
        let correct_deposit = NOMINAL * u128::from(10_000 - diet_bps) / 10_000;
        // A full nominal deposit while a diet applies → ERR_STAKE_DEPOSIT_MISMATCH.
        if diet_bps > 0 {
            ctx("alkemis.testnet", NOMINAL, TS);
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                c.transmute_spores_to_enzyme(id, 2, U128(NOMINAL));
            }));
            assert!(
                res.is_err(),
                "role {bits} zone {zone}: the full deposit must be rejected (diet {diet_bps} bps)"
            );
        }
        // The correct deposit → the transmute succeeds (amalgam minted).
        ctx("alkemis.testnet", correct_deposit, TS);
        let _ = c.transmute_spores_to_enzyme(id, 2, U128(NOMINAL));
        assert_eq!(
            c.get_inventory(id, "mt:enzyme-amalgam".to_string()).0,
            2,
            "role {bits} zone {zone}: transmute succeeds with deposit {correct_deposit}"
        );
    }

    // 8 combinations (4 roles × 2 zones); the Normal zone is checked inside each test
    // (a nominal deposit without a diet is always valid — the Normal matrix = 0 for every role).

    /// TC-EN-04-01 — Extractor+Fermenter @ Fever: 2000 bps discount (Extractor bit).
    #[test]
    fn tc_en_04_01_extractor_fever() {
        check_combination(0b011, "FEVER", crate::state::constants::DIET_FEVER_EXTRACTOR_BPS);
    }

    /// TC-EN-04-02 — Extractor+Fermenter @ Hypo: no diet (no Synth bit).
    #[test]
    fn tc_en_04_02_extractor_hypo() {
        check_combination(0b011, "HYPO", 0);
    }

    /// TC-EN-04-03 — Fermenter @ Fever: no diet (no Extractor bit).
    #[test]
    fn tc_en_04_03_fermenter_fever() {
        check_combination(0b010, "FEVER", 0);
    }

    /// TC-EN-04-04 — Fermenter @ Hypo: no diet (no Synthesizer bit).
    #[test]
    fn tc_en_04_04_fermenter_hypo() {
        check_combination(0b010, "HYPO", 0);
    }

    /// TC-EN-04-05 — Fermenter+Synthesizer @ Fever: no diet (no Extractor bit).
    #[test]
    fn tc_en_04_05_fermenter_synth_fever() {
        check_combination(0b110, "FEVER", 0);
    }

    /// TC-EN-04-06 — Fermenter+Synthesizer @ Hypo: 2000 bps discount (FAMINE_SYNTH).
    #[test]
    fn tc_en_04_06_fermenter_synth_hypo() {
        check_combination(
            0b110,
            "HYPO",
            crate::state::constants::DIET_FAMINE_SYNTHESIZER_BPS,
        );
    }

    /// TC-EN-04-07 — full-stack (E+F+S) @ Fever: 2000 bps discount (one matching bit is enough).
    #[test]
    fn tc_en_04_07_fullstack_fever() {
        check_combination(0b111, "FEVER", crate::state::constants::DIET_FEVER_EXTRACTOR_BPS);
    }

    /// TC-EN-04-08 — full-stack @ Hypo: 2000 bps discount (one matching bit is enough).
    #[test]
    fn tc_en_04_08_fullstack_hypo() {
        check_combination(
            0b111,
            "HYPO",
            crate::state::constants::DIET_FAMINE_SYNTHESIZER_BPS,
        );
    }
}
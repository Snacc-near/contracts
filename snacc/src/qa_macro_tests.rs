//! QA-115..119 (`TC-MACRO-*`) — Feature 4 test suite (the `CrucibleTemp` central bank):
//! - QA-115: Calcination Fever thermostat (`FEVER_BURN_BPS = 350` burn, `I-01` supply
//!   conservation, the `FEVER_THRESHOLD_MK = 610` boundary, `mt:elixir-homeostasis` shield,
//!   the `DK-033` 50/50 split of the non-burned fee).
//! - QA-116: Hypothermia thermostat (`REBATE_BPS = 200` rebate paid from the treasury, the
//!   `HYPOTHERMIA_THRESHOLD_MK = 600` boundary, the neutral `600..=610` band, the `I-04`
//!   `MAX_EPOCH_TREASURY_DRAIN_BPS = 500` quota per epoch and its reset in `advance_epoch`).
//! - QA-117: Rescue feeding (`EN-109`: `RESCUE_MIN_FEE_BPS = 2000` at the edge of death, the
//!   full fee at full vitality, linear in between; the 1-yocto fee floor; ATP is minted from
//!   the effective fee; starving cells cannot be fed).
//! - QA-118: Per-caste diet (`EN-110`/`EN-111`: Fever → Extractor bit, Hypothermia →
//!   Synthesizer bit, `20%` off the catalyst fee, never in the Normal band).
//! - QA-119: Nutrient Burst inside `feed_strain` (`EN-103`/`EN-104`: discount, the
//!   `I-04` quota cap shared with the burst accumulator, expiry after the epoch, and the
//!   composition order burst → rescue).
//!
//! Honest note: some behaviors (diet matrix, rescue bounds, burst expiry) are also touched by
//! older tests in `lib.rs`/`qa_diet_tests.rs`/`qa_en104_tests.rs`; they are re-asserted here
//! on purpose so every `TC-MACRO-*` case has its own traceable test. No contract behavior is
//! changed by this file.
#[cfg(test)]
#[allow(clippy::module_inception)]
mod qa_macro_tests {
    use crate::state::constants;
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::{get_logs, VMContextBuilder};
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    const ALKEMIS: &str = "alkemis.near";
    const INTRUDER: &str = "intruder.near";
    const CONTRACT_ID: &str = "snacc-v001.near";
    const T0: u64 = 1_000;
    /// Initial stake: supply 100_000 → strain 80_000, treasury 20_000 (`F1` 80/20 split).
    /// The `I-04` quota of epoch 0 is therefore `20_000 × 500 / 10_000 = 1_000`.
    const STAKE: u128 = 100_000;
    const QUOTA_EPOCH_0: u128 = 1_000;

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

    /// One strain (id 1) of the given role owned by `ALKEMIS`; dormancy disabled so the
    /// feed fee only depends on the rule under test.
    fn macro_setup(role: StrainRole) -> SnaccContract {
        ctx(ALKEMIS, 0, T0);
        let mut c = SnaccContract::new();
        ctx(ALKEMIS, STAKE, T0);
        let _ = c.incubate_strain(role, U128(STAKE), 0);
        c.macro_state.params.dormancy_threshold_bps = 0;
        c
    }

    /// Force `volume_epoch` so that `get_crucible_temp() == temp_mk` at `ts_sec`
    /// (temperature = `1000 × V / supply`; `last_update_ms = now` ⇒ zero decay).
    fn set_temp(c: &mut SnaccContract, temp_mk: u64, ts_sec: u64) {
        let supply = c.macro_state.total_snacc_supply;
        c.macro_state.volume_epoch = u128::from(temp_mk) * supply / 1000;
        c.macro_state.last_update_ms = ts_sec * 1000;
    }

    /// Same as `set_temp`, but the temperature is reached AFTER the fee joins `V_epoch`
    /// (the thermostat evaluates the zone with the fee already counted, `apply_thermostat_fee`).
    fn set_temp_after_fee(c: &mut SnaccContract, temp_mk: u64, fee: u128, ts_sec: u64) {
        set_temp(c, temp_mk, ts_sec);
        c.macro_state.volume_epoch -= fee;
    }

    fn extract(c: &mut SnaccContract, fee: u128, ts_sec: u64) {
        ctx(ALKEMIS, fee, ts_sec);
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(fee));
    }

    fn strain_snacc(c: &SnaccContract, id: u64) -> u128 {
        c.snacc_balances.get(&id).copied().unwrap_or(0)
    }

    fn logs() -> String {
        get_logs().join("\n")
    }

    fn set_vitality(c: &mut SnaccContract, vitality: u64, ts_sec: u64) {
        let mut v = c.strains.get(&1).unwrap().clone();
        v.vitality = vitality;
        v.last_fed_sec = ts_sec;
        let _ = c.strains.insert(1, v);
    }

    fn feed(c: &mut SnaccContract, amount: u128, ts_sec: u64) {
        ctx(ALKEMIS, 0, ts_sec);
        let _ = c.feed_strain(1, U128(amount));
    }

    // =========================================================================
    // QA-115 (TC-MACRO-01): Calcination Fever thermostat
    // =========================================================================

    /// QA-115: in Fever, 350 bps of the catalyst fee is burned. `I-01`: the burn is permanent —
    /// supply falls by exactly the burned amount and the burned counter rises by the same amount.
    #[test]
    fn tc_macro_01_fever_burns_350_bps_and_conserves_supply() {
        assert_eq!(constants::FEVER_BURN_BPS, 350);
        let mut c = macro_setup(StrainRole::Extractor);
        let fee = 10_000u128;
        set_temp_after_fee(&mut c, 800, fee, T0);
        let supply_before = c.macro_state.total_snacc_supply;
        let burned_before = c.macro_state.total_snacc_burned;
        let strain_before = strain_snacc(&c, 1);

        extract(&mut c, fee, T0);

        let burn = fee * 350 / 10_000;
        assert_eq!(burn, 350);
        assert_eq!(c.macro_state.total_snacc_supply, supply_before - burn);
        assert_eq!(c.macro_state.total_snacc_burned, burned_before + burn);
        // I-01: supply + burned is conserved by the thermostat.
        assert_eq!(
            c.macro_state.total_snacc_supply + c.macro_state.total_snacc_burned,
            supply_before + burned_before
        );
        // The catalyst fee is an external (attached) payment: the strain balance is untouched
        // and Fever never pays a rebate.
        assert_eq!(strain_snacc(&c, 1), strain_before);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        // The zone transition is announced (EN-106).
        let l = logs();
        assert!(l.contains(r#""event":"zone_changed""#));
        assert!(l.contains(r#""to_zone":"FEVER""#));
        assert!(l.contains(r#""crucible_temp":800"#));
    }

    /// QA-115: the non-burned part of a Fever fee follows `DK-033`: 50% to the dividend
    /// inflow, the rest to the `Rule-Bound Treasury`.
    #[test]
    fn tc_macro_01_fever_splits_net_fee_between_dividend_and_treasury() {
        let mut c = macro_setup(StrainRole::Extractor);
        let fee = 10_000u128;
        set_temp_after_fee(&mut c, 800, fee, T0);
        let treasury_before = c.treasury;

        extract(&mut c, fee, T0);

        let burn = fee * u128::from(constants::FEVER_BURN_BPS) / constants::BPS_DENOMINATOR;
        let net = fee - burn;
        let inflow = net * constants::DIVIDEND_INFLOW_BPS / constants::BPS_DENOMINATOR;
        assert_eq!(c.macro_state.epoch_dividend_inflow, inflow);
        assert_eq!(c.treasury, treasury_before + (net - inflow));
        assert_eq!(inflow + (net - inflow) + burn, fee, "every fee yocto is accounted for");
    }

    /// QA-115: `mt:elixir-homeostasis` (LC-11) halves the Fever burn per strain without being consumed.
    #[test]
    fn tc_macro_01_homeostasis_elixir_halves_the_burn() {
        let mut c = macro_setup(StrainRole::Extractor);
        let _ = c
            .inventories
            .insert((1, constants::ELIXIR_HOMEOSTASIS.to_string()), 1);
        let fee = 10_000u128;
        set_temp_after_fee(&mut c, 800, fee, T0);

        extract(&mut c, fee, T0);

        assert_eq!(c.macro_state.total_snacc_burned, 175, "350 bps × 50% = 175");
        assert_eq!(
            c.inventories
                .get(&(1, constants::ELIXIR_HOMEOSTASIS.to_string()))
                .copied(),
            Some(1),
            "the shield is passive: it is not consumed"
        );
    }

    /// QA-115: the Fever threshold is `610 mK` (strictly greater): 610 is Normal, 611 is Fever;
    /// the burn only starts at 611.
    #[test]
    fn tc_macro_01_fever_boundary_610_is_normal_611_is_fever() {
        assert_eq!(constants::FEVER_THRESHOLD_MK, 610);
        let fee = 10_000u128;
        for (temp, expect_zone, expect_burn) in [(610u64, "NORMAL", 0u128), (611, "FEVER", 350)] {
            let mut c = macro_setup(StrainRole::Extractor);
            set_temp(&mut c, temp, T0);
            assert_eq!(c.get_crucible_temp(), temp);
            assert_eq!(c.get_appetite_meter().zone, expect_zone, "temp {temp}");

            set_temp_after_fee(&mut c, temp, fee, T0);
            extract(&mut c, fee, T0);
            assert_eq!(
                c.macro_state.total_snacc_burned, expect_burn,
                "temp {temp} burn"
            );
        }
    }

    // =========================================================================
    // QA-116 (TC-MACRO-02/03): Hypothermia thermostat, neutral band, I-04 quota
    // =========================================================================

    /// QA-116: in Hypothermia the strain receives a 200 bps rebate funded by the treasury.
    /// Nothing is burned and the supply is unchanged.
    #[test]
    fn tc_macro_02_hypothermia_rebate_200_bps_from_treasury() {
        assert_eq!(constants::REBATE_BPS, 200);
        let mut c = macro_setup(StrainRole::Extractor);
        let fee = 10_000u128;
        set_temp_after_fee(&mut c, 500, fee, T0);
        let supply_before = c.macro_state.total_snacc_supply;
        let treasury_before = c.treasury;
        let strain_before = strain_snacc(&c, 1);

        extract(&mut c, fee, T0);

        let rebate = fee * 200 / 10_000;
        assert_eq!(rebate, 200);
        let net = fee;
        let inflow = net * constants::DIVIDEND_INFLOW_BPS / constants::BPS_DENOMINATOR;
        assert_eq!(strain_snacc(&c, 1), strain_before + rebate, "rebate credited");
        assert_eq!(
            c.treasury,
            treasury_before + (net - inflow) - rebate,
            "rebate is paid by the treasury"
        );
        assert_eq!(c.macro_state.epoch_treasury_spent, rebate);
        assert_eq!(c.macro_state.total_snacc_supply, supply_before, "no burn in Hypothermia");
        assert_eq!(c.macro_state.total_snacc_burned, 0);
        assert!(logs().contains(r#""to_zone":"HYPO""#));
    }

    /// QA-116: the neutral band `600..=610 mK` pays neither burn nor rebate; 599 is already Hypothermia.
    #[test]
    fn tc_macro_02_normal_band_has_no_burn_and_no_rebate() {
        assert_eq!(constants::HYPOTHERMIA_THRESHOLD_MK, 600);
        let fee = 10_000u128;
        for (temp, expect_zone, expect_rebate) in [
            (599u64, "HYPO", true),
            (600, "NORMAL", false),
            (605, "NORMAL", false),
            (610, "NORMAL", false),
        ] {
            let mut c = macro_setup(StrainRole::Extractor);
            set_temp(&mut c, temp, T0);
            assert_eq!(c.get_appetite_meter().zone, expect_zone, "temp {temp}");

            set_temp_after_fee(&mut c, temp, fee, T0);
            let strain_before = strain_snacc(&c, 1);
            extract(&mut c, fee, T0);
            assert_eq!(c.macro_state.total_snacc_burned, 0, "temp {temp}: no burn");
            if expect_rebate {
                assert!(strain_snacc(&c, 1) > strain_before, "temp {temp}: rebate");
            } else {
                assert_eq!(strain_snacc(&c, 1), strain_before, "temp {temp}: no rebate");
                assert_eq!(c.macro_state.epoch_treasury_spent, 0);
            }
        }
    }

    /// QA-116 (`I-04`): the rebate is capped by `500 bps` of the epoch's initial treasury; a second
    /// call in the same epoch finds the quota exhausted.
    #[test]
    fn tc_macro_03_rebate_capped_by_i04_quota_per_epoch() {
        assert_eq!(constants::MAX_EPOCH_TREASURY_DRAIN_BPS, 500);
        let mut c = macro_setup(StrainRole::Extractor);
        assert_eq!(
            c.macro_state.epoch_treasury_initial * 500 / 10_000,
            QUOTA_EPOCH_0
        );
        let fee = 55_000u128; // raw rebate 1_100 > quota 1_000
        let strain_before = strain_snacc(&c, 1);

        set_temp_after_fee(&mut c, 550, fee, T0);
        extract(&mut c, fee, T0);
        assert_eq!(strain_snacc(&c, 1), strain_before + QUOTA_EPOCH_0, "clamped to the quota");
        assert_eq!(c.macro_state.epoch_treasury_spent, QUOTA_EPOCH_0);

        // Quota exhausted: the same low-temperature fee pays nothing more this epoch.
        set_temp_after_fee(&mut c, 550, fee, T0);
        extract(&mut c, fee, T0);
        assert_eq!(strain_snacc(&c, 1), strain_before + QUOTA_EPOCH_0);
        assert_eq!(c.macro_state.epoch_treasury_spent, QUOTA_EPOCH_0);
        assert!(
            c.macro_state.epoch_treasury_spent
                <= c.macro_state.epoch_treasury_initial * 500 / 10_000,
            "I-04 never exceeded"
        );
    }

    /// QA-116 (`I-04`): `advance_epoch` resets the spent counter and re-snapshots the treasury,
    /// so rebates flow again in the new epoch — and the epoch cannot be closed early.
    #[test]
    fn tc_macro_03_quota_resets_on_advance_epoch() {
        let mut c = macro_setup(StrainRole::Extractor);
        let fee = 55_000u128;
        set_temp_after_fee(&mut c, 550, fee, T0);
        extract(&mut c, fee, T0);
        assert_eq!(c.macro_state.epoch_treasury_spent, QUOTA_EPOCH_0);

        // Time gate: closing the epoch before it ends is rejected.
        ctx(ALKEMIS, 0, T0 + 10);
        let early = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.advance_epoch();
        }));
        assert!(early.is_err(), "ERR_EPOCH_NOT_ENDED expected");

        let t1 = T0 + constants::EPOCH_DURATION_SEC;
        ctx(ALKEMIS, 0, t1);
        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.macro_state.epoch_treasury_initial, c.treasury);

        set_vitality(&mut c, 10_000, t1);
        let strain_before = strain_snacc(&c, 1);
        set_temp_after_fee(&mut c, 550, fee, t1);
        extract(&mut c, fee, t1);
        let raw_rebate = fee * 200 / 10_000;
        assert_eq!(
            strain_snacc(&c, 1),
            strain_before + raw_rebate,
            "new epoch: the larger quota no longer clamps the rebate"
        );
    }

    // =========================================================================
    // QA-117 (TC-MACRO-04): Rescue feeding
    // =========================================================================

    /// QA-117: the rescue multiplier is linear in effective vitality from 20% (edge of death)
    /// to 100% (full vitality); the fee debited is `amount × multiplier`.
    #[test]
    fn tc_macro_04_rescue_fee_is_linear_from_20_to_100_percent() {
        assert_eq!(constants::RESCUE_MIN_FEE_BPS, 2000);
        let amount = 10_000u128;
        let mut previous = 0u128;
        for (eff, expected_bps) in [
            (1u64, 2_000u128),
            (2_500, 4_000),
            (5_000, 6_000),
            (7_500, 8_000),
            (10_000, 10_000),
        ] {
            let mut c = macro_setup(StrainRole::Extractor);
            set_vitality(&mut c, eff, T0);
            feed(&mut c, amount, T0);
            let expected_fee = amount * expected_bps / 10_000;
            let l = logs();
            assert!(
                l.contains(&format!(r#""rescue_bps":{expected_bps},"#)),
                "eff {eff}: rescue_bps"
            );
            assert!(
                l.contains(&format!(r#""effective_fee":{expected_fee},"#)),
                "eff {eff}: effective_fee"
            );
            assert!(expected_fee > previous, "closer to full vitality ⇒ higher fee");
            previous = expected_fee;
        }
    }

    /// QA-117: a rescue-feed debits the strain by the effective fee (minus whatever the
    /// thermostat rebates back), and recovers metabolism (+500 vitality, `last_fed_sec = now`).
    #[test]
    fn tc_macro_04_rescue_debits_effective_fee_and_recovers_vitality() {
        let mut c = macro_setup(StrainRole::Extractor);
        set_vitality(&mut c, 5_000, T0);
        let before = strain_snacc(&c, 1);
        feed(&mut c, 10_000, T0);
        let effective = 6_000u128; // 10_000 × 6000 / 10000
        assert_eq!(
            strain_snacc(&c, 1),
            before - effective + c.macro_state.epoch_treasury_spent,
            "debit = effective fee − thermostat rebate"
        );
        let v = c.get_strain(1);
        assert_eq!(v.vitality, 5_500, "metabolic recovery +500");
        assert_eq!(v.last_fed_sec, T0);
    }

    /// QA-117 (DK-030): the fee floor — the combination of rescue × upkeep can never make a feed free.
    #[test]
    fn tc_macro_04_fee_floor_is_one_yocto() {
        let mut c = macro_setup(StrainRole::Extractor);
        set_vitality(&mut c, 1, T0);
        feed(&mut c, 1, T0); // 1 × 2000/10000 = 0 → floored to 1
        assert!(logs().contains(r#""effective_fee":1,"#));
    }

    /// QA-117: ATP is minted from the EFFECTIVE (post-rescue) fee, never from the nominal amount.
    #[test]
    fn tc_macro_04_atp_minted_from_effective_fee() {
        let mut c = macro_setup(StrainRole::Extractor);
        c.set_strain_snacc(1, 5 * 10u128.pow(15));
        c.macro_state.total_snacc_supply = 10u128.pow(24);
        set_vitality(&mut c, 5_000, T0);
        let atp_before = c.get_strain(1).atp_balance;
        // nominal 2e15 = 2 ATP, effective 1.2e15 (rescue 60%) = 1 ATP
        feed(&mut c, 2 * constants::ATP_YOCTO_UNIT, T0);
        assert_eq!(c.get_strain(1).atp_balance, atp_before + 1);
        assert!(logs().contains(r#""atp_minted":1"#));
    }

    /// QA-117: a starving cell (effective vitality 0) cannot be rescue-fed.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn tc_macro_04_starving_cell_cannot_be_fed() {
        let mut c = macro_setup(StrainRole::Extractor);
        set_vitality(&mut c, 0, T0);
        feed(&mut c, 1_000, T0);
    }

    /// QA-117: only the strain owner can feed it.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn tc_macro_04_non_owner_cannot_feed() {
        let mut c = macro_setup(StrainRole::Extractor);
        ctx(INTRUDER, 0, T0);
        let _ = c.feed_strain(1, U128(1_000));
    }

    /// QA-117: feeding more than the strain balance is rejected.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn tc_macro_04_feed_above_balance_rejected() {
        let mut c = macro_setup(StrainRole::Extractor);
        let above_balance = strain_snacc(&c, 1) + 1;
        feed(&mut c, above_balance, T0);
    }

    // =========================================================================
    // QA-118 (TC-MACRO-05): per-caste diet
    // =========================================================================

    /// Run one `transmute_spores_to_enzyme` batch for a strain whose role mask is `mask` while
    /// the temperature BEFORE the fee is `temp_mk`; returns the contract and the emitted logs.
    fn transmute_case(mask: u8, temp_mk: u64, deposit: u128) -> (SnaccContract, String) {
        let mut c = macro_setup(StrainRole::Fermenter);
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = mask;
        let _ = c.strains.insert(1, v);
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 2);
        set_temp(&mut c, temp_mk, T0);
        ctx(ALKEMIS, deposit, T0);
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(NOMINAL_FEE));
        let l = logs();
        (c, l)
    }

    const NOMINAL_FEE: u128 = 10_000;

    /// QA-118: the diet matrix over the three zones × the four Fermenter-bearing masks.
    /// Fever + Extractor bit → 20% off; Hypothermia + Synthesizer bit → 20% off; the Normal band
    /// never has a diet. The deposit must equal the fee AFTER the discount.
    #[test]
    fn tc_macro_05_diet_matrix_by_zone_and_caste() {
        assert_eq!(constants::DIET_FEVER_EXTRACTOR_BPS, 2000);
        assert_eq!(constants::DIET_FAMINE_SYNTHESIZER_BPS, 2000);
        // (mask, temperature before the fee, expected diet bps)
        let cases: [(u8, u64, u16); 12] = [
            (0b010, 800, 0),
            (0b011, 800, 2_000),
            (0b110, 800, 0),
            (0b111, 800, 2_000),
            (0b010, 0, 0),
            (0b011, 0, 0),
            (0b110, 0, 2_000),
            (0b111, 0, 2_000),
            (0b010, 605, 0),
            (0b011, 605, 0),
            (0b110, 605, 0),
            (0b111, 605, 0),
        ];
        for (mask, temp, diet_bps) in cases {
            let deposit = NOMINAL_FEE * u128::from(10_000 - diet_bps) / 10_000;
            let (_c, l) = transmute_case(mask, temp, deposit);
            assert!(
                l.contains(&format!(r#""diet_bps":{diet_bps}"#)),
                "mask {mask:#05b} @ {temp} mK: diet_bps {diet_bps} expected"
            );
            assert!(
                l.contains(&format!(r#""catalyst_fee":{deposit},"#)),
                "mask {mask:#05b} @ {temp} mK: fee after diet {deposit}"
            );
        }
    }

    /// QA-118: the diet reduces the fee that reaches the thermostat, so the Fever burn shrinks with it
    /// (8_000 × 350 bps = 280, not 350).
    #[test]
    fn tc_macro_05_diet_discount_lowers_the_fever_burn() {
        let (c, _l) = transmute_case(0b011, 800, 8_000);
        assert_eq!(c.macro_state.total_snacc_burned, 8_000 * 350 / 10_000);
        let (c_plain, _l) = transmute_case(0b010, 800, 10_000);
        assert_eq!(c_plain.macro_state.total_snacc_burned, 10_000 * 350 / 10_000);
    }

    /// QA-118: paying the nominal fee while a diet applies is rejected.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn tc_macro_05_nominal_deposit_rejected_when_diet_applies() {
        let _ = transmute_case(0b011, 800, NOMINAL_FEE);
    }

    // =========================================================================
    // QA-119 (TC-MACRO-06): Nutrient Burst inside feed_strain
    // =========================================================================

    fn arm_burst(c: &mut SnaccContract, bps: u16, until_epoch: u64) {
        c.macro_state.nutrient_burst_bps = bps;
        c.macro_state.nutrient_burst_until = until_epoch;
    }

    /// QA-119: an active burst discounts the feed fee; the discount remainder stays in the strain balance.
    #[test]
    fn tc_macro_06_burst_discount_applies_on_feed() {
        let mut c = macro_setup(StrainRole::Extractor);
        arm_burst(&mut c, 1_000, 1);
        set_vitality(&mut c, 10_000, T0);
        assert_eq!(c.get_burst_state().0 .0, 1_000);

        feed(&mut c, 10_000, T0);

        assert!(logs().contains(r#""burst_bps":1000,"#));
        assert!(logs().contains(r#""effective_fee":9000,"#));
        assert_eq!(c.macro_state.epoch_burst_discount_granted, 1_000);

        // Control: the same feed without a burst costs the full 10_000.
        let mut control = macro_setup(StrainRole::Extractor);
        set_vitality(&mut control, 10_000, T0);
        feed(&mut control, 10_000, T0);
        assert!(logs().contains(r#""effective_fee":10000,"#));
        assert_eq!(control.macro_state.epoch_burst_discount_granted, 0);
    }

    /// QA-119 (`EN-104`): the discount is capped by the `I-04` quota (shared with the rebate
    /// spend) and by the per-epoch accumulator; once spent, the next feed pays in full.
    #[test]
    fn tc_macro_06_burst_capped_by_i04_quota_and_accumulator() {
        let mut c = macro_setup(StrainRole::Extractor);
        arm_burst(&mut c, constants::MAX_BURST_BPS, 1);
        set_vitality(&mut c, 10_000, T0);

        // raw discount 2_000 (20%) > quota 1_000 → clamped to the quota.
        feed(&mut c, 10_000, T0);
        assert!(logs().contains(r#""burst_bps":2000,"#));
        assert!(logs().contains(r#""effective_fee":9000,"#));
        assert_eq!(c.macro_state.epoch_burst_discount_granted, QUOTA_EPOCH_0);

        // Quota consumed (accumulator + the thermostat rebate): the next feed gets no discount.
        feed(&mut c, 10_000, T0);
        assert!(logs().contains(r#""effective_fee":10000,"#));
        assert_eq!(c.macro_state.epoch_burst_discount_granted, QUOTA_EPOCH_0);
    }

    /// QA-119: the burst is keeperless — it expires when the epoch advances, and the
    /// accumulator resets with the new epoch.
    #[test]
    fn tc_macro_06_burst_expires_after_epoch_and_accumulator_resets() {
        let mut c = macro_setup(StrainRole::Extractor);
        arm_burst(&mut c, 1_000, 1);
        set_vitality(&mut c, 10_000, T0);
        feed(&mut c, 10_000, T0);
        assert_eq!(c.macro_state.epoch_burst_discount_granted, 1_000);

        let t1 = T0 + constants::EPOCH_DURATION_SEC;
        ctx(ALKEMIS, 0, t1);
        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.macro_state.epoch_burst_discount_granted, 0, "accumulator reset");
        assert_eq!(c.get_burst_state().0 .0, 0, "burst expired with the epoch");
        assert_eq!(c.get_appetite_meter().burst_bps, 0);

        set_vitality(&mut c, 10_000, t1);
        feed(&mut c, 10_000, t1);
        assert!(logs().contains(r#""burst_bps":0,"#));
        assert!(logs().contains(r#""effective_fee":10000,"#));
    }

    /// QA-119: composition order is burst → rescue (both only ever shrink the fee):
    /// `(amount − burst discount) × rescue_bps / 10000`.
    #[test]
    fn tc_macro_06_burst_then_rescue_compose() {
        let mut c = macro_setup(StrainRole::Extractor);
        arm_burst(&mut c, 1_000, 1);
        set_vitality(&mut c, 5_000, T0); // rescue 6000 bps
        feed(&mut c, 10_000, T0);
        let l = logs();
        assert!(l.contains(r#""burst_bps":1000,"#));
        assert!(l.contains(r#""rescue_bps":6000,"#));
        // (10_000 − 1_000) × 6_000 / 10_000
        assert!(l.contains(r#""effective_fee":5400,"#));
    }
}

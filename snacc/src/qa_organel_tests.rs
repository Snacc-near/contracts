//! EN-119 — per-slot organel boundary tests (`tc_organel_01..03`): a cost of 1 ATP per
//! level increase, level ≤ `MAX_ORGANEL_LEVEL = 5`, 1×/epoch, and merge inheritance.
//! Per-slot economic effects: **only Mitochondria is specified** (the ATP
//! §13a formula); Nucleus & Lysosome are **not yet specified** in the vault (see
//! `docs/organel-effects.md`) — the tests here lock the BOUNDARIES, not the effects.
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so the test paths do not change
mod qa_organel_tests {
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use crate::StrainId;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    fn acct(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }
    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct("snacc-v001.testnet"))
            .signer_account_id(acct(predecessor))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }
    fn fresh_contract(stake: u128, role: StrainRole, spores: u64) -> (SnaccContract, StrainId) {
        ctx("alkemis.testnet", stake, 1_000);
        let mut c = SnaccContract::new();
        let v = c.incubate_strain(role, U128(stake), spores);
        (c, v.strain_id)
    }

    /// TC-ORGANEL-01 — Mitochondria: levels up to 5 (a cumulative cost of 1+2+3+4+5 = 15),
    /// level 6 is rejected with `ERR_ORGANEL_LEVEL_MAX`; the +10%/level ATP bonus shows up in the formula.
    #[test]
    fn tc_organel_01_mitochondria_boundaries() {
        let (mut c, id) = fresh_contract(10u128.pow(24), StrainRole::Extractor, 0);
        // The cost = 1 ATP per level INCREASE: 0→1→2→3→4→5 = 5 increases × 1 ATP.
        for level in 1..=5u8 {
            if level > 1 {
                c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch(); // the epoch moves via advance_epoch, not wall time
            }
            ctx("alkemis.testnet", 0, 1_000);
            let v = c.allocate_atp(id, level, 0, 0);
            assert_eq!(v.organel.mitokondria, level);
            assert_eq!(v.atp_balance, 50 - u128::from(level), "cumulative cost {level}");
        }
        // Level 6 → rejected.
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.allocate_atp(id, 6, 0, 0);
        }));
        assert!(res.is_err(), "level 6 must ERR_ORGANEL_LEVEL_MAX");
    }

    /// TC-ORGANEL-02 — Nucleus: the same level bounds & costs; **no additional economic
    /// effect** (not yet specified — `docs/organel-effects.md`); the value is
    /// stored and shown in the view.
    #[test]
    fn tc_organel_02_nucleus_boundaries() {
        let (mut c, id) = fresh_contract(10u128.pow(24), StrainRole::Extractor, 0);
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.allocate_atp(id, 0, 5, 0);
        assert_eq!(v.organel.nukleus, 5);
        assert_eq!(v.atp_balance, 45, "a cost of 5 ATP (1 per level)");
        // No observable secondary effect beyond storage —
        // atp_balance only changes by this allocation cost.
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let v2 = c.allocate_atp(id, 0, 3, 0); // lower the level → free
        assert_eq!(v2.atp_balance, 45, "lowering a level is free");
        assert_eq!(v2.organel.nukleus, 3);
    }

    /// TC-ORGANEL-03 — Lysosome: the same level bounds & costs; **no additional economic
    /// effect** (not yet specified); raising/lowering levels and zeroing.
    #[test]
    fn tc_organel_03_lysosome_boundaries() {
        let (mut c, id) = fresh_contract(10u128.pow(24), StrainRole::Extractor, 0);
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.allocate_atp(id, 0, 0, 4);
        assert_eq!(v.organel.lisosom, 4);
        assert_eq!(v.atp_balance, 46);
        // Raise to 5 in the next epoch (a cost of 1).
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let v2 = c.allocate_atp(id, 0, 0, 5);
        assert_eq!(v2.atp_balance, 45);
        // Reset to 0 (free) — a slot can be emptied.
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let v3 = c.allocate_atp(id, 0, 0, 0);
        assert_eq!(v3.organel.lisosom, 0);
        assert_eq!(v3.atp_balance, 45);
    }
    /// TC-ORGANEL-04 (EN-119 Nucleus) — the transmute & synthesize output rises +10%/level:
    /// nucleus 5 → ×1.5; nucleus 0 → ×1. Verifies the `minted = base × (10000 +
    /// 1000×nuk)/10000` formula and that level 6 is still rejected.
    #[test]
    fn tc_organel_04_nucleus_production_output() {
        // Two twin-efficiency strains (10800 via 1 splice): A = F+S (nucleus 5),
        // B = F+E (nucleus 0) — the only variable = the nucleus.
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut ca = SnaccContract::new();
        let ida = ca.incubate_strain(StrainRole::Fermenter, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let ds = ca.incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.splice_strains(ida, ds);
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut cb = SnaccContract::new();
        let idb = cb.incubate_strain(StrainRole::Fermenter, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let de = cb.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 0, 1_000);
        let _ = cb.splice_strains(idb, de);
        assert_eq!(ca.get_strain(ida).efficiency_bps, cb.get_strain(idb).efficiency_bps, "equal efficiency");

        // A: nucleus 5 (a cost of 5 ATP) → minted ×1.5.
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.allocate_atp(ida, 0, 5, 0);
        let _ = ca.inventories.insert((ida, "mt:spore-ignis".to_string()), 8);
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.transmute_spores_to_enzyme(ida, 8, U128(0));
        assert_eq!(
            ca.get_inventory(ida, "mt:enzyme-amalgam".to_string()).0,
            12,
            "nucleus 5 → 8×1.08×1.5 = 12.96 → 12"
        );
        // B: nucleus 0 → the base minted.
        let _ = cb.inventories.insert((idb, "mt:spore-ignis".to_string()), 8);
        ctx("alkemis.testnet", 0, 1_000);
        let _ = cb.transmute_spores_to_enzyme(idb, 8, U128(0));
        assert_eq!(
            cb.get_inventory(idb, "mt:enzyme-amalgam".to_string()).0,
            8,
            "nucleus 0 → 8×1.08×1.0 = 8.64 → 8"
        );
        // Synthesize: the nucleus also multiplies elixirs (2 batches × 1.5 = 3).
        ca.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        ca.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.inventories.insert((ida, "mt:enzyme-amalgam".to_string()), 4);
        let _ = ca.inventories.insert((ida, "mt:enzyme-synthase".to_string()), 2);
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.synthesize_elixir(ida, "mt:elixir-homunculus".to_string(), 6, U128(0));
        assert_eq!(
            ca.get_inventory(ida, "mt:elixir-homunculus".to_string()).0,
            3,
            "elixirs 2 batches × 1.5 (nucleus 5) = 3"
        );
    }

    #[test]
    fn tc_organel_05_lysosome_longer_lifespan() {
        let (mut c, id) = fresh_contract(10u128.pow(24), StrainRole::Extractor, 0);
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.allocate_atp(id, 0, 0, 4); // lysosome 4 → effective BMR 60
        assert_eq!(v.organel.lisosom, 4);
        let vitality0 = v.vitality; // 8500
        // 70 hours: the base BMR 100×70 = 7000 → eff 1500; with the lysosome: 60×70 = 4200 → eff 4300.
        ctx("alkemis.testnet", 0, 1_000 + 70 * 3_600);
        let v70 = c.get_strain(id);
        assert_eq!(
            v70.effective_vitality,
            vitality0 - 60 * 70,
            "lysosome 4 → effective BMR 60/h (survives longer; without it it would already be 1500/Distressed)"
        );
        // Bound: lysosome 5 → effective BMR 50 (the min via the level cap; not 0).
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000 + 70 * 3_600);
        let v5 = c.allocate_atp(id, 0, 0, 5);
        assert_eq!(v5.organel.lisosom, 5);
        ctx("alkemis.testnet", 0, 1_000 + 70 * 3_600);
        let v5l = c.get_strain(id);
        assert_eq!(
            v5l.effective_vitality,
            v5.vitality - 50 * 70,
            "lysosome 5 → effective BMR 50/h (never 0 — the level cap is 5)"
        );
    }
}

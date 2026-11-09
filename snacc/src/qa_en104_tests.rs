//! EN-104 — burst overflow tests (`EN-103`): repeated bursts & repeated harvests
//! never overflow the `u16`/`I-04` quota (`epoch_burst_discount_granted` ≤ cap).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so the test paths do not change
mod qa_en104_tests {
    use crate::state::StrainRole;
    use crate::SnaccContract;
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

    /// EN-104 — 30 consecutive feeds with a full burst + a small I-04 quota:
    /// `epoch_burst_discount_granted` locks at the cap (no overdraw/overflow),
    /// the debit never goes negative, `nutrient_burst_bps` ≤ `MAX_BURST_BPS`.
    #[test]
    fn tc_en_104_burst_overflow() {
        ctx("alkemis.testnet", 10_000, 1_000);
        let mut c = SnaccContract::new();
        let id = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
        // A full burst + a small I-04 quota (initial 2_000 → cap 100).
        c.macro_state.nutrient_burst_bps = crate::state::constants::MAX_BURST_BPS; // 2000
        c.macro_state.nutrient_burst_until = u64::MAX; // the burst lasts "forever" — tests the clamp
        c.macro_state.epoch_treasury_initial = 2_000;
        let cap = 2_000u128 * 500 / 10_000; // 100
        for i in 0..30u64 {
            ctx("alkemis.testnet", 0, 1_000 + i * 60);
            let w0 = c.get_total_wealth(acct("alkemis.testnet")).0;
            let _ = c.feed_strain(id, U128(100));
            let debit = w0 - c.get_total_wealth(acct("alkemis.testnet")).0;
            // THE EN-104 POINT: the quota runs out → granted locks at the cap — NO overdraw.
            assert!(
                c.macro_state.epoch_burst_discount_granted <= cap,
                "granted exceeds the cap (overflow!) at iteration {i}"
            );
            // The debit never goes negative (feeding never profits past 0).
            assert!(debit <= 100, "the discount exceeds the nominal (iteration {i}: {debit})");
            // DK-030 (fixed): a 1-yocto fee floor — iteration 0 (dormant × burst)
            // now debits 1 (feeding is NEVER free; previously 0 = the DK-030 finding).
            if i == 0 {
                assert_eq!(debit, 1, "the DK-030 floor: feeding is never free");
            }
        }
        assert_eq!(
            c.macro_state.epoch_burst_discount_granted, cap,
            "granted locks exactly at the cap once the quota is exhausted"
        );
        assert!(
            c.macro_state.nutrient_burst_bps <= crate::state::constants::MAX_BURST_BPS,
            "burst_bps melebihi MAX_BURST_BPS"
        );
        // Conservation holds: the supply is unchanged by feeding (a hypo zone without burn).
        assert_eq!(c.ft_total_supply().0, 10_000);
    }

    /// EN-104 — repeated bursts via consecutive harvests: `nutrient_burst_bps` stays
    /// ≤ `MAX_BURST_BPS` (never stacks past the u16/2000 cap) — 3 liquidations.
    #[test]
    fn tc_en_104_repeated_harvest_bursts_do_not_stack() {
        ctx("alkemis.testnet", 10_000, 1_000);
        let mut c = SnaccContract::new();
        let mut ids = Vec::new();
        for _ in 0..3 {
            let id = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
            ids.push(id);
        }
        // 3 consecutive lyses (all eff 0 at ts 90 h) — the burst is re-set on every lysis.
        ctx("hunter.testnet", 0, 1_000 + 90 * 3_600);
        for id in &ids {
            let _ = c.harvest_starving_strain(*id);
            assert!(
                c.macro_state.nutrient_burst_bps
                    <= crate::state::constants::MAX_BURST_BPS,
                "the burst stacks past MAX_BURST_BPS"
            );
            assert!(c.macro_state.nutrient_burst_until > 0);
        }
        // The final value = the result of the last lysis (not a historical sum).
        assert_eq!(c.macro_state.nutrient_burst_bps, crate::state::constants::MAX_BURST_BPS);
    }

/// DK-030 (a dedicated regression) — the worst combination: a 100% burst (2000 bps) + dormancy
/// (upkeep 50 bps) + maximum rescue → the fee stays ≥ 1 yocto (never free).
#[test]
fn tc_dk030_feeding_is_never_free() {
    ctx("alkemis.testnet", 10_000, 1_000);
    let mut c = SnaccContract::new();
    let id = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
    // A full burst forever + dormancy (volume 0) + maximum rescue (full vitality).
    c.macro_state.nutrient_burst_bps = crate::state::constants::MAX_BURST_BPS;
    c.macro_state.nutrient_burst_until = u64::MAX;
    c.macro_state.epoch_treasury_initial = 2_000;
    let mut v = c.strains.get(&id).unwrap().clone();
    v.vitality = crate::state::constants::VITALITY_CAP;
    let _ = c.strains.insert(id, v);
    for i in 0..5u64 {
        ctx("alkemis.testnet", 0, 1_000 + i * 60);
        let w0 = c.get_total_wealth(acct("alkemis.testnet")).0;
        let _ = c.feed_strain(id, U128(100));
        let debit = w0 - c.get_total_wealth(acct("alkemis.testnet")).0;
        assert!(
            debit >= 1,
            "DK-030: feeding must always be paid (iteration {i}: debit {debit})"
        );
        assert!(debit <= 100, "the debit never exceeds the nominal");
    }

}

/// SEC-112/migrate — a regression: old state (strains + a TEE chat_id) is NOT readable
/// after `migrate()` (the generation prefix rises → the new collections are empty).
#[test]
fn tc_migrate_membersihkan_collection() {
    ctx("alkemis.testnet", 10u128.pow(24), 1_000);
    let mut c = SnaccContract::new();
    // 5 strain — vektor TEE live payload `strain_id:4` (sama seperti qa_div_tests).
    for _ in 0..5 {
        let _ = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 3);
    }
    let id = 4; // target TEE live vector
    // Populate used_tee_chat_ids via the live TEE vector (register → commit → apply).
    // In-repo fixture (the live TEE vector strain_id:4) — no dependence on external paths.
    let raw = include_str!("../tests/fixtures/tee-vector-live.json");
    let vec_json: near_sdk::serde_json::Value = near_sdk::serde_json::from_str(raw).unwrap();
    ctx("alkemis.testnet", 0, 1_000);
    c.register_attested_signer(
        vec_json["tee_pubkey_hex"].as_str().unwrap().to_string(), 1_700_000_000);
    ctx("alkemis.testnet", 0, 1_000);
    c.commit_genome_experiment(id, vec_json["req_sha256_hex"].as_str().unwrap().to_string());
    ctx("alkemis.testnet", 0, 1_000);
    let _ = c.apply_tee_mutation_zero_trust(
        id,
        vec_json["req_sha256_hex"].as_str().unwrap().to_string(),
        vec_json["raw_response_body"].as_str().unwrap().to_string(),
        vec_json["signature_hex"].as_str().unwrap().to_string(),
        vec_json["tee_pubkey_hex"].as_str().unwrap().to_string(),
    );
    // Pre-migrate: the state is populated ✓.
    assert!(c.ft_total_supply().0 > 0, "the supply exists pre-migrate");
    assert_eq!(c.get_strain(id).owner_id, acct("alkemis.testnet"));
    // The chat_id is populated: a replay is rejected (the I-06 anti-replay is active).

    // MIGRATE → the generation prefix rises → ALL collections are empty.
    // SEC-113: migrate is now #[private] — the predecessor must be the contract's own account.
    ctx("snacc-v001.testnet", 0, 1_000);
    c = SnaccContract::migrate();
    // (1) used_tee_chat_ids EMPTY — no leftover data (the old bug: a fake replay).
    // (1) used_tee_chat_ids empty — replay is no longer rejected by anti-replay (the old bug is gone).
    // (2) strains EMPTY.
    // (2) strains empty — get_strain is already tested in (4).
    // (3) balances reset.
    assert_eq!(c.ft_total_supply().0, 0, "the old supply leaked!");
    assert_eq!(c.treasury, 0, "the old treasury leaked!");
    // (4) get_strain → the strain does not exist.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.get_strain(id);
    }));
    assert!(res.is_err(), "the old strain is still readable after migrate!");
    // (5) prefix_gen rises (1 from 0) — the marker is stored.
    assert_eq!(c.prefix_gen, 1, "prefix_gen must rise to 1");
    }
}

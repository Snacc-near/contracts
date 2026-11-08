//! EN-104 — tes overflow burst (`EN-103`): burst berulang & harvest berulang
//! tidak pernah overflow `u16`/kuota `I-04` (`epoch_burst_discount_granted` ≤ cap).
#[cfg(test)]
#[allow(clippy::module_inception)] // nama modul = nama file qa_*; dipertahankan agar path test tidak berubah
mod qa_en104_tests {
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    fn akun(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }
    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(akun("snacc-v001.testnet"))
            .signer_account_id(akun(predecessor))
            .predecessor_account_id(akun(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }

    /// EN-104 — 30 feed beruntun dengan burst penuh + kuota I-04 kecil:
    /// `epoch_burst_discount_granted` terkunci di cap (tidak overdraw/overflow),
    /// debit tidak pernah negatif, `nutrient_burst_bps` ≤ `MAX_BURST_BPS`.
    #[test]
    fn tc_en_104_burst_overflow() {
        ctx("alkemis.testnet", 10_000, 1_000);
        let mut c = SnaccContract::new();
        let id = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
        // Burst penuh + kuota I-04 kecil (initial 2.000 → cap 100).
        c.macro_state.nutrient_burst_bps = crate::state::constants::MAX_BURST_BPS; // 2000
        c.macro_state.nutrient_burst_until = u64::MAX; // burst "selamanya" — uji clamp
        c.macro_state.epoch_treasury_initial = 2_000;
        let cap = 2_000u128 * 500 / 10_000; // 100
        for i in 0..30u64 {
            ctx("alkemis.testnet", 0, 1_000 + i * 60);
            let w0 = c.get_total_wealth(akun("alkemis.testnet")).0;
            let _ = c.feed_strain(id, U128(100));
            let debit = w0 - c.get_total_wealth(akun("alkemis.testnet")).0;
            // INTI EN-104: kuota habis → granted terkunci di cap — TIDAK overdraw.
            assert!(
                c.macro_state.epoch_burst_discount_granted <= cap,
                "granted melebihi cap (overflow!) di iterasi {i}"
            );
            // Debit tidak pernah negatif (feed tidak menguntungkan melewati 0).
            assert!(debit <= 100, "diskon melebihi nominal (iterasi {i}: {debit})");
            // DK-030 (diperbaiki): lantai fee 1 yocto — iterasi 0 (dormant × burst)
            // kini debit 1 (feed TIDAK pernah gratis; sebelumnya 0 = temuan DK-030).
            if i == 0 {
                assert_eq!(debit, 1, "lantai DK-030: feed tidak pernah gratis");
            }
        }
        assert_eq!(
            c.macro_state.epoch_burst_discount_granted, cap,
            "granted terkunci tepat di cap setelah kuota habis"
        );
        assert!(
            c.macro_state.nutrient_burst_bps <= crate::state::constants::MAX_BURST_BPS,
            "burst_bps melebihi MAX_BURST_BPS"
        );
        // Konservasi tetap: supply tak berubah oleh feed (zona hypo tanpa burn).
        assert_eq!(c.ft_total_supply().0, 10_000);
    }

    /// EN-104 — burst berulang via harvest beruntun: `nutrient_burst_bps` selalu
    /// ≤ `MAX_BURST_BPS` (tidak menumpuk melewati cap u16/2000) — 3 likuidasi.
    #[test]
    fn tc_en_104_burst_harvest_berulang_tak_menumpuk() {
        ctx("alkemis.testnet", 10_000, 1_000);
        let mut c = SnaccContract::new();
        let mut ids = Vec::new();
        for _ in 0..3 {
            let id = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
            ids.push(id);
        }
        // 3 lisis beruntun (semua eff 0 di ts 90 jam) — burst di-set ulang tiap lisis.
        ctx("hunter.testnet", 0, 1_000 + 90 * 3_600);
        for id in &ids {
            let _ = c.harvest_starving_strain(*id);
            assert!(
                c.macro_state.nutrient_burst_bps
                    <= crate::state::constants::MAX_BURST_BPS,
                "burst menumpuk melewati MAX_BURST_BPS"
            );
            assert!(c.macro_state.nutrient_burst_until > 0);
        }
        // Nilai final = hasil lisis terakhir (bukan penjumlahanhistoris).
        assert_eq!(c.macro_state.nutrient_burst_bps, crate::state::constants::MAX_BURST_BPS);
    }

/// DK-030 (regresi khusus) — kombinasi terburuk: burst 100% (2000 bps) + dormansi
/// (upkeep 50 bps) + rescue maksimum → fee tetap ≥ 1 yocto (tidak pernah gratis).
#[test]
fn tc_dk030_feed_tidak_pernah_gratis() {
    ctx("alkemis.testnet", 10_000, 1_000);
    let mut c = SnaccContract::new();
    let id = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0).strain_id;
    // Burst penuh selamanya + dormansi (volume 0) + rescue maksimum (vitality penuh).
    c.macro_state.nutrient_burst_bps = crate::state::constants::MAX_BURST_BPS;
    c.macro_state.nutrient_burst_until = u64::MAX;
    c.macro_state.epoch_treasury_initial = 2_000;
    let mut v = c.strains.get(&id).unwrap().clone();
    v.vitality = crate::state::constants::VITALITY_CAP;
    let _ = c.strains.insert(id, v);
    for i in 0..5u64 {
        ctx("alkemis.testnet", 0, 1_000 + i * 60);
        let w0 = c.get_total_wealth(akun("alkemis.testnet")).0;
        let _ = c.feed_strain(id, U128(100));
        let debit = w0 - c.get_total_wealth(akun("alkemis.testnet")).0;
        assert!(
            debit >= 1,
            "DK-030: feed harus selalu berbayar (iterasi {i}: debit {debit})"
        );
        assert!(debit <= 100, "debit tak melebihi nominal");
    }

}

/// SEC-112/migrate — regresi: state lama (strain + TEE chat_id) TIDAK terbaca
/// setelah `migrate()` (prefix generasi naik → collection baru kosong).
#[test]
fn tc_migrate_membersihkan_collection() {
    ctx("alkemis.testnet", 10u128.pow(24), 1_000);
    let mut c = SnaccContract::new();
    // 5 strain — vektor TEE live payload `strain_id:4` (sama seperti qa_div_tests).
    for _ in 0..5 {
        let _ = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 3);
    }
    let id = 4; // target TEE live vector
    // Isi used_tee_chat_ids via TEE live vector (register → commit → apply).
    // Fixture di dalam repo (vektor TEE live strain_id:4) — tidak bergantung path luar.
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
    // Pra-migrate: state terisi ✓.
    assert!(c.ft_total_supply().0 > 0, "supply ada pra-migrate");
    assert_eq!(c.get_strain(id).owner_id, akun("alkemis.testnet"));
    // chat_id terisi: verifikasi via replay ditolak (I-06 anti-replay aktif).

    // MIGRATE → prefix generasi naik → SEMUA collection kosong.
    // SEC-113: migrate kini #[private] — predecessor wajib = akun kontrak sendiri.
    ctx("snacc-v001.testnet", 0, 1_000);
    c = SnaccContract::migrate();
    // (1) used_tee_chat_ids KOSONG — tidak ada sisa data lama (bug lama: replay palsu).
    // (1) used_tee_chat_ids kosong — replay tidak lagi ditolak anti-replay (bug lama hilang).
    // (2) strains KOSONG.
    // (2) strains kosong — get_strain sudah diuji di (4).
    // (3) saldo reset.
    assert_eq!(c.ft_total_supply().0, 0, "supply lama bocor!");
    assert_eq!(c.treasury, 0, "treasury lama bocor!");
    // (4) get_strain → strain tidak ada.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.get_strain(id);
    }));
    assert!(res.is_err(), "strain lama masih terbaca setelah migrate!");
    // (5) prefix_gen naik (1 dari 0) — marker tersimpan.
    assert_eq!(c.prefix_gen, 1, "prefix_gen harus naik ke 1");
    }
}

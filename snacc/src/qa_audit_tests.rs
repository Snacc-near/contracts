//! Audit kelengkapan 2026-11-08 (W4–W10) — test perbaikan kontrak.
//! Mandiri (helpers lokal `ctx`/`baru` — pola qa_f1_tests).
#[cfg(test)]
#[allow(clippy::module_inception)] // nama modul = nama file qa_*
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
    const LAIN: &str = "penyusup.testnet";
    const T0: u64 = 1_000;

    fn akun(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }
    fn ctx_on(contract: &str, predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(akun(contract))
            .signer_account_id(akun(predecessor))
            .predecessor_account_id(akun(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }
    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        ctx_on("snacc-v001.testnet", predecessor, deposit_yocto, ts_sec);
    }
    /// Kontrak baru (owner = `OWNER`) + satu strain ber-stake NEAR.
    fn baru(stake: u128, role: StrainRole, spores: u64) -> (SnaccContract, StrainId) {
        ctx(OWNER, stake, T0);
        let mut c = SnaccContract::new();
        let v = c.incubate_strain(role, U128(stake), spores);
        (c, v.strain_id)
    }

    // ------------------------------------------------------------------
    // W9 — `advance_sandbox_clock` (ADR-22) + `now_sec()` tunggal
    // ------------------------------------------------------------------

    /// Owner memajukan jam 48 jam → BMR/aging ikut (Extractor 100/jam).
    #[test]
    fn w9_sandbox_clock_memajukan_aging() {
        let (mut c, id) = baru(10_000, StrainRole::Extractor, 0);
        assert!(c.sandbox_clock_enabled, "akun .testnet = mode sandbox");
        assert_eq!(c.get_strain(id).effective_vitality, 8500);
        ctx(OWNER, 0, T0);
        assert_eq!(c.advance_sandbox_clock(48), 48 * 3600);
        assert_eq!(c.get_strain(id).effective_vitality, 8500 - 48 * 100);
        let (enabled, offset, now) = c.get_sandbox_clock();
        assert!(enabled);
        assert_eq!(offset, 48 * 3600);
        assert_eq!(now, T0 + 48 * 3600);
    }

    /// Tier 2 → hostile terbuka, Tier 3 → lisis terbuka tanpa menunggu jam nyata.
    #[test]
    fn w9_sandbox_clock_membuka_tier2_dan_lisis() {
        let (mut c, id) = baru(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(70); // 8500 - 7000 = 1500 → Tier 2
        assert_eq!(c.get_strain(id).effective_vitality, 1500);
        let _ = c.advance_sandbox_clock(15); // → 0 → Tier 3
        assert_eq!(c.get_strain(id).effective_vitality, 0);
        ctx(LAIN, 0, T0);
        let (bounty, _) = c.harvest_starving_strain(id);
        assert!(bounty.0 > 0);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_CONTRACT_OWNER")]
    fn w9_sandbox_clock_bukan_owner_ditolak() {
        let (mut c, _) = baru(10_000, StrainRole::Extractor, 0);
        ctx(LAIN, 0, T0);
        let _ = c.advance_sandbox_clock(1);
    }

    /// Akun mainnet (`.near`) → flag mati sejak init; tak ada setter.
    #[test]
    #[should_panic(expected = "ERR_SANDBOX_CLOCK_DISABLED")]
    fn w9_sandbox_clock_mati_di_mainnet() {
        ctx_on("snacc-v001.near", OWNER, 0, T0);
        let mut c = SnaccContract::new();
        assert!(!c.sandbox_clock_enabled);
        let _ = c.advance_sandbox_clock(1);
    }

    /// `migrate()` di akun mainnet juga mematikan flag.
    #[test]
    fn w9_migrate_mainnet_flag_mati_testnet_hidup() {
        ctx_on("snacc-v001.near", "snacc-v001.near", 0, T0);
        assert!(!SnaccContract::migrate().sandbox_clock_enabled);
        ctx_on("snacc-v001.testnet", "snacc-v001.testnet", 0, T0);
        let m = SnaccContract::migrate();
        assert!(m.sandbox_clock_enabled);
        assert_eq!(m.sandbox_time_offset_sec, 0);
    }

    #[test]
    #[should_panic(expected = "ERR_BAD_AMOUNT")]
    fn w9_sandbox_clock_nol_jam_ditolak() {
        let (mut c, _) = baru(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(0);
    }

    #[test]
    #[should_panic(expected = "ERR_BAD_AMOUNT")]
    fn w9_sandbox_clock_melebihi_batas_ditolak() {
        let (mut c, _) = baru(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(constants::MAX_SANDBOX_ADVANCE_HOURS + 1);
    }

    // ------------------------------------------------------------------
    // W4 — cek pemilik pada produksi (V-01, ERR_NOT_STRAIN_OWNER)
    // ------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn w4_extract_bukan_pemilik_ditolak() {
        let (mut c, id) = baru(10_000, StrainRole::Extractor, 0);
        ctx(LAIN, 0, T0);
        let _ = c.extract_spores(id, "mt:spore-ignis".to_string(), U128(0));
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn w4_transmute_bukan_pemilik_ditolak() {
        let (mut c, id) = baru(10_000, StrainRole::Fermenter, 0);
        let _ = c.inventories.insert((id, "mt:spore-ignis".to_string()), 4);
        ctx(LAIN, 0, T0);
        let _ = c.transmute_spores_to_enzyme(id, 2, U128(0));
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn w4_synthesize_bukan_pemilik_ditolak() {
        let (mut c, id) = baru(10_000, StrainRole::Synthesizer, 0);
        let _ = c.inventories.insert((id, constants::ENZYME_AMALGAM.to_string()), 4);
        let _ = c.inventories.insert((id, constants::ENZYME_SYNTHASE.to_string()), 4);
        ctx(LAIN, 0, T0);
        let _ = c.synthesize_elixir(id, constants::ELIXIR_HOMUNCULUS.to_string(), 3, U128(0));
    }

    /// Pemilik tetap bisa (regresi positif) dan ATP korban tidak tersentuh oleh akun lain.
    #[test]
    fn w4_pemilik_tetap_bisa_extract() {
        let (mut c, id) = baru(10_000, StrainRole::Extractor, 0);
        let atp0 = c.get_strain(id).atp_balance;
        ctx(OWNER, 0, T0);
        let v = c.extract_spores(id, "mt:spore-ignis".to_string(), U128(0));
        assert_eq!(v.atp_balance, atp0 - 1);
    }

    // ------------------------------------------------------------------
    // W5 — tutup mint gratis `incubate_strain` (I-01) + W7 onboarding
    // ------------------------------------------------------------------

    /// Deposit 0 + stake N tanpa saldo akun → ditolak; supply tidak berubah.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn w5_stake_tanpa_deposit_tanpa_saldo_ditolak() {
        let (mut c, _) = baru(10_000, StrainRole::Extractor, 0);
        ctx(LAIN, 0, T0);
        let _ = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0);
    }

    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn w5_stake_nol_deposit_nol_ditolak() {
        ctx(OWNER, 0, T0);
        let mut c = SnaccContract::new();
        let _ = c.incubate_strain(StrainRole::Extractor, U128(0), 0);
    }

    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn w5_deposit_beda_dengan_stake_ditolak() {
        ctx(OWNER, 5_000, T0);
        let mut c = SnaccContract::new();
        let _ = c.incubate_strain(StrainRole::Extractor, U128(10_000), 0);
    }

    /// W7 — pemain baru hanya punya NEAR: hatch ber-deposit = jalur onboarding;
    /// suplai lahir = deposit, 80% ke strain, bisa langsung `feed_strain`; V-04 solvent.
    #[test]
    fn w7_onboarding_pemain_baru_dengan_near() {
        ctx(OWNER, 0, T0);
        let mut c = SnaccContract::new();
        let stake = 10u128.pow(24);
        ctx("pemain-baru.testnet", stake, T0);
        let v = c.incubate_strain(StrainRole::Fermenter, U128(0), 2);
        assert_eq!(c.ft_total_supply().0, stake);
        assert_eq!(c.snacc_balances.get(&v.strain_id).copied(), Some(stake * 8 / 10));
        ctx("pemain-baru.testnet", 0, T0);
        let _ = c.feed_strain(v.strain_id, U128(10u128.pow(21)));
        assert!(c.get_global_solvency().solvent);
    }

    /// Stake dari saldo akun (bounty lisis) → supply TIDAK bertambah; saldo akun
    /// didebit; invarian V-04 tetap solvent di semua jalur.
    #[test]
    fn w5_stake_dari_saldo_akun_tanpa_mint() {
        let (mut c, id) = baru(10u128.pow(24), StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(100); // eff 0 → Tier 3
        ctx(LAIN, 0, T0);
        let (bounty, _) = c.harvest_starving_strain(id);
        assert!(bounty.0 > 0);
        assert_eq!(c.ft_balance_of(akun(LAIN)), bounty);
        let supply0 = c.ft_total_supply().0;
        assert!(c.get_global_solvency().solvent);

        let v = c.incubate_strain(StrainRole::Synthesizer, bounty, 0);
        assert_eq!(c.ft_total_supply().0, supply0, "tanpa mint baru");
        assert_eq!(c.ft_balance_of(akun(LAIN)).0, 0);
        let to_treasury = bounty.0 * 2000 / 10_000;
        assert_eq!(
            c.snacc_balances.get(&v.strain_id).copied(),
            Some(bounty.0 - to_treasury)
        );
        assert!(c.get_global_solvency().solvent);
    }

    // ------------------------------------------------------------------
    // W6 — gerbang waktu `advance_epoch`
    // ------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "ERR_EPOCH_NOT_ENDED")]
    fn w6_advance_epoch_sebelum_berakhir_ditolak() {
        let (mut c, _) = baru(10_000, StrainRole::Extractor, 0);
        ctx(LAIN, 0, T0 + constants::EPOCH_DURATION_SEC - 1);
        c.advance_epoch();
    }

    /// Tepat di batas → lolos; panggilan kedua di blok yang sama → ditolak (anti-spam).
    #[test]
    fn w6_advance_epoch_setelah_berakhir_lalu_spam_ditolak() {
        let (mut c, _) = baru(10_000, StrainRole::Extractor, 0);
        let t1 = T0 + constants::EPOCH_DURATION_SEC;
        ctx(LAIN, 0, t1);
        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.macro_state.epoch_ends_sec, t1 + constants::EPOCH_DURATION_SEC);
        let spam = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.advance_epoch()));
        assert!(spam.is_err());
    }

    /// Jam sandbox (ADR-22) ikut membuka gerbang epoch.
    #[test]
    fn w6_advance_epoch_lewat_sandbox_clock() {
        let (mut c, _) = baru(10_000, StrainRole::Extractor, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(constants::EPOCH_DURATION_SEC / 3600);
        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.get_macro_state().epoch_ends_sec, c.macro_state.epoch_ends_sec);
    }

    // ------------------------------------------------------------------
    // Dividen — overflow u128 `inflow × 1e18` di advance_epoch (bug live)
    // ------------------------------------------------------------------

    #[test]
    fn div_mul_div_u128_eksak_dan_saturasi() {
        use crate::mul_div_u128;
        assert_eq!(mul_div_u128(10, 20, 3), 66);
        assert_eq!(mul_div_u128(10u128.pow(30), 10u128.pow(18), 10u128.pow(12)), 10u128.pow(36));
        assert_eq!(mul_div_u128(u128::MAX, u128::MAX, u128::MAX), u128::MAX);
        assert_eq!(mul_div_u128(u128::MAX, 2, 4), u128::MAX / 2);
        assert_eq!(mul_div_u128(10u128.pow(30), 10u128.pow(18), 1), u128::MAX);
        assert_eq!(mul_div_u128(1, 1, 0), 0);
    }

    /// Inflow 1e21..1e30 (live: 3.537e23) → epoch tertutup + klaim end-to-end
    /// membayar seluruh inflow ke penyumbang tunggal; V-04 solvent.
    #[test]
    fn div_inflow_besar_epoch_tertutup_dan_klaim() {
        let (mut c, id) = baru(10u128.pow(24), StrainRole::Synthesizer, 0);
        for (epoch, exp) in [21u32, 23, 24, 27, 30].into_iter().enumerate() {
            let _ = c
                .inventories
                .insert((id, constants::ELIXIR_AETHER.to_string()), 3);
            ctx(OWNER, 0, T0);
            c.synthesize_and_redeem_aether(id, U128(2));
            // Simulasikan inflow besar (dicatat sebagai suplai beredar → V-04 seimbang).
            let inflow = 10u128.pow(exp);
            c.macro_state.total_snacc_supply += inflow;
            c.macro_state.epoch_dividend_inflow += inflow;
            c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
            c.advance_epoch(); // dulu: panic overflow
            let payout = c.claim_industrial_dividend(id, epoch as u64);
            assert_eq!(payout.0, inflow, "exp={exp}");
            assert!(c.get_global_solvency().solvent, "exp={exp}");
        }
    }

    // ------------------------------------------------------------------
    // W10 — lifecycle: splice BMR −15%, status dominan, MAX_GENERATION, Distressed
    // ------------------------------------------------------------------

    /// Dua strain milik OWNER (dom Synthesizer BMR 150, abs Extractor).
    fn dua_strain() -> (SnaccContract, StrainId, StrainId) {
        let (mut c, dom) = baru(10_000, StrainRole::Synthesizer, 0);
        ctx(OWNER, 10_000, T0);
        let abs = c.incubate_strain(StrainRole::Extractor, U128(0), 0).strain_id;
        (c, dom, abs)
    }

    #[test]
    fn w10_splice_bmr_turun_15_persen_dan_vitalitas_disettle() {
        let (mut c, dom, abs) = dua_strain();
        ctx(OWNER, 0, T0 + 10 * 3600); // 10 jam: dom eff = 8500 - 10×150 = 7000
        let v = c.splice_strains(dom, abs);
        assert_eq!(v.bmr_per_hour, 150 * 8500 / 10_000); // 127
        assert_eq!(v.vitality, 7000, "settle ke now sebelum BMR baru");
        assert_eq!(v.last_fed_sec, T0 + 10 * 3600);
        assert_eq!(v.effective_vitality, 7000);
        assert_eq!(v.generation, 1);
        // BMR baru berlaku ke depan: +10 jam → 7000 - 10×127.
        ctx(OWNER, 0, T0 + 20 * 3600);
        assert_eq!(c.get_strain(dom).effective_vitality, 7000 - 10 * 127);
    }

    #[test]
    #[should_panic(expected = "ERR_STRAIN_ALREADY_SPLICED")]
    fn w10_splice_dominan_sudah_terserap_ditolak() {
        let (mut c, dom, abs) = dua_strain();
        ctx(OWNER, 10_000, T0);
        let ketiga = c.incubate_strain(StrainRole::Fermenter, U128(0), 0).strain_id;
        ctx(OWNER, 0, T0);
        let _ = c.splice_strains(dom, abs); // abs → SplicedBurned
        let _ = c.splice_strains(abs, ketiga); // abs sebagai dominan → ditolak
    }

    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn w10_splice_dominan_tier3_ditolak() {
        let (mut c, dom) = baru(10_000, StrainRole::Synthesizer, 0);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(60); // dom eff = 8500 - 9000 → 0
        ctx(OWNER, 10_000, T0);
        let abs = c.incubate_strain(StrainRole::Extractor, U128(0), 0).strain_id;
        ctx(OWNER, 0, T0);
        let _ = c.splice_strains(dom, abs);
    }

    #[test]
    #[should_panic(expected = "ERR_MAX_GENERATION")]
    fn w10_splice_max_generation_ditolak() {
        let (mut c, dom, abs) = dua_strain();
        let mut v = c.strains.get(&dom).unwrap().clone();
        v.generation = constants::MAX_GENERATION;
        let _ = c.strains.insert(dom, v);
        ctx(OWNER, 0, T0);
        let _ = c.splice_strains(dom, abs);
    }

    #[test]
    #[should_panic(expected = "ERR_MAX_GENERATION")]
    fn w10_hostile_max_generation_ditolak() {
        // Target milik LAIN menua ke Tier 2; raider OWNER generasi 8.
        ctx(OWNER, 0, T0);
        let mut c = SnaccContract::new();
        ctx(LAIN, 10_000, T0);
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

    /// View `status` & `get_solvency_tier` mengikuti tangga I-02 lewat Lazy BMR.
    #[test]
    fn w10_view_distressed_dan_solvency_tier() {
        use crate::state::{SolvencyTier, StrainStatus};
        let (mut c, id) = baru(10_000, StrainRole::Extractor, 0);
        assert_eq!(c.get_strain(id).status, StrainStatus::Active);
        assert_eq!(c.get_solvency_tier(id), SolvencyTier::Tier1Healthy);
        ctx(OWNER, 0, T0);
        let _ = c.advance_sandbox_clock(65); // 8500 - 6500 = 2000 → Tier 2 (batas)
        assert_eq!(c.get_strain(id).status, StrainStatus::Distressed);
        assert_eq!(c.get_solvency_tier(id), SolvencyTier::Tier2Chapter11Distressed);
        let _ = c.advance_sandbox_clock(20); // → 0
        assert_eq!(c.get_solvency_tier(id), SolvencyTier::Tier3Chapter7Insolvent);
        ctx(LAIN, 0, T0);
        let _ = c.harvest_starving_strain(id);
        assert_eq!(c.get_strain(id).status, StrainStatus::Liquidated);
    }

    // ------------------------------------------------------------------
    // W8 — nama sub-account unik lintas generasi state
    // ------------------------------------------------------------------

    fn ctx_h(contract: &str, predecessor: &str, deposit: u128, height: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(akun(contract))
            .predecessor_account_id(akun(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .block_height(height)
            .block_timestamp(T0 * 1_000_000_000);
        testing_env!(b.build());
    }

    /// Reset state (migrate) → `next_strain_id` mulai ulang dari 1, tetapi nama
    /// sub-account strain 1 berbeda dari generasi sebelumnya (tidak bentrok).
    #[test]
    fn w8_subaccount_unik_lintas_migrate() {
        const K: &str = "snacc-v001.testnet";
        ctx_h(K, OWNER, 0, 100);
        let mut c = SnaccContract::new();
        ctx_h(K, OWNER, 10_000, 100);
        let lama = c.incubate_strain(StrainRole::Extractor, U128(0), 0);
        assert_eq!(lama.vault_subaccount.to_string(), "s-1-100.snacc-v001.testnet");

        ctx_h(K, K, 0, 250);
        let mut m = SnaccContract::migrate();
        ctx_h(K, OWNER, 10_000, 251);
        let baru = m.incubate_strain(StrainRole::Extractor, U128(0), 0);
        assert_eq!(baru.strain_id, 1, "id mulai ulang setelah reset state");
        assert_eq!(baru.vault_subaccount.to_string(), "s-1-250.snacc-v001.testnet");
        assert_ne!(lama.vault_subaccount, baru.vault_subaccount);
    }

    /// `materialize_subaccount` memakai nama dari ledger (sama dengan view), bukan
    /// merakit ulang `s-<id>` lama.
    #[test]
    fn w8_materialize_memakai_nama_ledger() {
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

    /// Pro-rata dua penyumbang (2:1) pada inflow besar — eksak tanpa skala 1e18.
    #[test]
    fn div_pro_rata_dua_penyumbang_inflow_besar() {
        let (mut c, a) = baru(10u128.pow(24), StrainRole::Synthesizer, 0);
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

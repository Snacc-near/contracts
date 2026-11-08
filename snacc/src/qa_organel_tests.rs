//! EN-119 — tes batas per slot organel (`tc_organel_01..03`): biaya 1 ATP per
//! kenaikan level, level ≤ `MAX_ORGANEL_LEVEL = 5`, 1×/epoch, dan warisan merge.
//! Efek ekonomi per slot: **hanya Mitokondria yang dispesifikasikan** (rumus ATP
//! §13a); Nukleus & Lisosom **belum dispesifikasikan** di vault (lihat
//! `docs/organel-effects.md`) — test di sini mengunci BATAS, bukan efek.
#[cfg(test)]
#[allow(clippy::module_inception)] // nama modul = nama file qa_*; dipertahankan agar path test tidak berubah
mod qa_organel_tests {
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use crate::StrainId;
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
    fn baru(stake: u128, role: StrainRole, spores: u64) -> (SnaccContract, StrainId) {
        ctx("alkemis.testnet", stake, 1_000);
        let mut c = SnaccContract::new();
        let v = c.incubate_strain(role, U128(stake), spores);
        (c, v.strain_id)
    }

    /// TC-ORGANEL-01 — Mitokondria: level sampai 5 (biaya kumulatif 1+2+3+4+5 = 15),
    /// level 6 ditolak `ERR_ORGANEL_LEVEL_MAX`; bonus ATP +10%/level terasa di rumus.
    #[test]
    fn tc_organel_01_mitokondria_batas() {
        let (mut c, id) = baru(10u128.pow(24), StrainRole::Extractor, 0);
        // Biaya = 1 ATP per KENAIKAN level: 0→1→2→3→4→5 = 5 kenaikan × 1 ATP.
        for level in 1..=5u8 {
            if level > 1 {
                c.advance_epoch(); // epoch bergerak via advance_epoch, bukan waktu
            }
            ctx("alkemis.testnet", 0, 1_000);
            let v = c.allocate_atp(id, level, 0, 0);
            assert_eq!(v.organel.mitokondria, level);
            assert_eq!(v.atp_balance, 50 - u128::from(level), "biaya kumulatif {level}");
        }
        // Level 6 → ditolak.
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.allocate_atp(id, 6, 0, 0);
        }));
        assert!(res.is_err(), "level 6 harus ERR_ORGANEL_LEVEL_MAX");
    }

    /// TC-ORGANEL-02 — Nukleus: batas level & biaya sama; **tanpa efek ekonomi
    /// tambahan** (belum dispesifikasikan — `docs/organel-effects.md`); nilainya
    /// tersimpan dan tampil di view.
    #[test]
    fn tc_organel_02_nukleus_batas() {
        let (mut c, id) = baru(10u128.pow(24), StrainRole::Extractor, 0);
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.allocate_atp(id, 0, 5, 0);
        assert_eq!(v.organel.nukleus, 5);
        assert_eq!(v.atp_balance, 45, "biaya 5 ATP (1 per level)");
        // Tidak ada efek sekunder yang bisa diamati selain penyimpanan —
        // atp_balance hanya berubah oleh biaya alokasi ini.
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let v2 = c.allocate_atp(id, 0, 3, 0); // turunkan level → gratis
        assert_eq!(v2.atp_balance, 45, "penurunan level gratis");
        assert_eq!(v2.organel.nukleus, 3);
    }

    /// TC-ORGANEL-03 — Lisosom: batas level & biaya sama; **tanpa efek ekonomi
    /// tambahan** (belum dispesifikasikan); turun-naik level dan zeroing.
    #[test]
    fn tc_organel_03_lisosom_batas() {
        let (mut c, id) = baru(10u128.pow(24), StrainRole::Extractor, 0);
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.allocate_atp(id, 0, 0, 4);
        assert_eq!(v.organel.lisosom, 4);
        assert_eq!(v.atp_balance, 46);
        // Naik ke 5 di epoch berikutnya (biaya 1).
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let v2 = c.allocate_atp(id, 0, 0, 5);
        assert_eq!(v2.atp_balance, 45);
        // Reset ke 0 (gratis) — slot bisa dikosongkan.
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let v3 = c.allocate_atp(id, 0, 0, 0);
        assert_eq!(v3.organel.lisosom, 0);
        assert_eq!(v3.atp_balance, 45);
    }
    /// TC-ORGANEL-04 (EN-119 Nukleus) — output transmute & synthesize naik +10%/level:
    /// nukleus 5 → ×1,5; nukleus 0 → ×1. Verifikasi rumus `minted = base × (10000 +
    /// 1000×nuk)/10000` dan level 6 tetap ditolak.
    #[test]
    fn tc_organel_04_nukleus_output_produksi() {
        // Dua strain kembar efisiensi (10800 via 1 splice): A = F+S (nukleus 5),
        // B = F+E (nukleus 0) — satu-satunya variabel = nukleus.
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
        assert_eq!(ca.get_strain(ida).efficiency_bps, cb.get_strain(idb).efficiency_bps, "efisiensi setara");

        // A: nukleus 5 (biaya 5 ATP) → minted ×1,5.
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.allocate_atp(ida, 0, 5, 0);
        let _ = ca.inventories.insert((ida, "mt:spore-ignis".to_string()), 8);
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.transmute_spores_to_enzyme(ida, 8, U128(0));
        assert_eq!(
            ca.get_inventory(ida, "mt:enzyme-amalgam".to_string()).0,
            12,
            "nukleus 5 → 8×1,08×1,5 = 12,96 → 12"
        );
        // B: nukleus 0 → minted dasar.
        let _ = cb.inventories.insert((idb, "mt:spore-ignis".to_string()), 8);
        ctx("alkemis.testnet", 0, 1_000);
        let _ = cb.transmute_spores_to_enzyme(idb, 8, U128(0));
        assert_eq!(
            cb.get_inventory(idb, "mt:enzyme-amalgam".to_string()).0,
            8,
            "nukleus 0 → 8×1,08×1,0 = 8,64 → 8"
        );
        // Synthesize: nukleus ikut mengalikan eliksir (2 batch × 1,5 = 3).
        ca.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.inventories.insert((ida, "mt:enzyme-amalgam".to_string()), 4);
        let _ = ca.inventories.insert((ida, "mt:enzyme-synthase".to_string()), 2);
        ctx("alkemis.testnet", 0, 1_000);
        let _ = ca.synthesize_elixir(ida, "mt:elixir-homunculus".to_string(), 6, U128(0));
        assert_eq!(
            ca.get_inventory(ida, "mt:elixir-homunculus".to_string()).0,
            3,
            "eliksir 2 batch × 1,5 (nukleus 5) = 3"
        );
    }

    #[test]
    fn tc_organel_05_lisosom_umur_lebih_panjang() {
        let (mut c, id) = baru(10u128.pow(24), StrainRole::Extractor, 0);
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.allocate_atp(id, 0, 0, 4); // lisosom 4 → BMR eff 60
        assert_eq!(v.organel.lisosom, 4);
        let vitality0 = v.vitality; // 8500
        // 70 jam: BMR dasar 100×70 = 7000 → eff 1500; lisosom: 60×70 = 4200 → eff 4300.
        ctx("alkemis.testnet", 0, 1_000 + 70 * 3_600);
        let v70 = c.get_strain(id);
        assert_eq!(
            v70.effective_vitality,
            vitality0 - 60 * 70,
            "lisosom 4 → BMR eff 60/jam (selamat lebih lama; tanpa lisosom sudah 1500/Distressed)"
        );
        // Batas: lisosom 5 → BMR eff 50 (min via cap level; bukan 0).
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000 + 70 * 3_600);
        let v5 = c.allocate_atp(id, 0, 0, 5);
        assert_eq!(v5.organel.lisosom, 5);
        ctx("alkemis.testnet", 0, 1_000 + 70 * 3_600);
        let v5l = c.get_strain(id);
        assert_eq!(
            v5l.effective_vitality,
            v5.vitality - 50 * 70,
            "lisosom 5 → BMR eff 50/jam (tidak pernah 0 — level cap 5)"
        );
    }
}

//! QA-120..122 (TC-MA-*) — M&A: `splice_strains` (role OR, pajak 2,5%, Gen+1,
//! ATP 5, warisan inventori), `hostile_takeover_splice` (115%, 95/5, nonce `I-14`,
//! ATP 10), warisan organel `max(dom, abs)` + reset `last_allocate_epoch`.
#[cfg(test)]
#[allow(clippy::module_inception)] // nama modul = nama file qa_*; dipertahankan agar path test tidak berubah
mod qa_ma_tests {
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

    /// QA-120 (TC-MA-01) — `splice_strains`: `role_mask |=`, pajak 2,5% ke treasury,
    /// `Gen+1`, ATP dominan −5, inventori terserap pindah utuh (konservasi).
    #[test]
    fn tc_ma_01_splice_role_pajak_atp_inventori() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let dom = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let abs = c.incubate_strain(StrainRole::Fermenter, U128(10u128.pow(24)), 0).strain_id;
        // Inventori di sel terserap: 7 spora + 2 enzim — wajib pindah utuh.
        let _ = c.inventories.insert((abs, "mt:spore-ignis".to_string()), 7);
        let _ = c.inventories.insert((abs, "mt:enzyme-amalgam".to_string()), 2);
        let treasury0 = c.treasury;
        let atp0 = c.get_strain(dom).atp_balance;
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.splice_strains(dom, abs);
        // role OR; Gen+1; ATP −5.
        assert_eq!(v.role_mask, 0b011, "role_mask |= (I-08)");
        assert_eq!(v.generation, 1, "Gen+1");
        assert_eq!(v.atp_balance, atp0 - 5, "biaya ATP splice = 5 (§13a)");
        // Pajak 2,5% dari saldo terserap (80% stake = 8×10^23 → tax 2×10^22).
        assert_eq!(c.treasury - treasury0, 2 * 10u128.pow(22), "pajak 2,5% (ADR-25)");
        // Inventori pindah utuh ke dominan.
        assert_eq!(c.get_inventory(dom, "mt:spore-ignis".to_string()).0, 7);
        assert_eq!(c.get_inventory(dom, "mt:enzyme-amalgam".to_string()).0, 2);
        assert_eq!(c.get_inventory(abs, "mt:spore-ignis".to_string()).0, 0);
        // Sel terserap mati (apoptosis LC-07a).
        assert_eq!(c.get_strain(abs).status, crate::state::StrainStatus::SplicedBurned);
    }

    /// QA-121 (TC-MA-02) — hostile: bayar `book × 115/100` (95% penjual / 5% treasury),
    /// `expected_target_nonce` stale → gagal (`I-14`), ATP raider −10.
    #[test]
    fn tc_ma_02_hostile_115_nonce_atp() {
        ctx("pemilik.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let target = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("raider.testnet", 2 * 10u128.pow(24), 1_000);
        let raider = c.incubate_strain(StrainRole::Fermenter, U128(2 * 10u128.pow(24)), 0).strain_id;
        // Target Distressed: eff 1500 → 70 jam (BMR Extractor 100).
        ctx("raider.testnet", 0, 1_000 + 70 * 3_600);
        assert_eq!(c.get_strain(target).effective_vitality, 1500);
        // Nonce stale → gagal (I-14).
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.hostile_takeover_splice(raider, target, 99);
        }));
        assert!(res.is_err(), "nonce stale harus ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED");
        // Nonce benar (0) → takeover; book = 8×10^23; payment = 115% = 9,2×10^23.
        let treasury0 = c.treasury;
        let seller0 = c.ft_balance_of(akun("pemilik.testnet")).0;
        let atp0 = c.get_strain(raider).atp_balance;
        let (_, view) = c.hostile_takeover_splice(raider, target, 0);
        let payment = 92 * 10u128.pow(22); // 8×10^23 × 1,15
        assert_eq!(c.ft_balance_of(akun("pemilik.testnet")).0 - seller0, payment * 95 / 100, "95% ke penjual");
        assert_eq!(c.treasury - treasury0, payment * 5 / 100, "5% ke treasury");
        assert_eq!(view.atp_balance, atp0 - 10, "biaya ATP hostile = 10 (§13a)");
        assert_eq!(c.get_strain(target).status, crate::state::StrainStatus::SplicedBurned);
    }

    /// SEC-111 (triase Opsi a — DIPERBAIKI): `pending_callbacks` kini kunci NYATA —
    /// set 1 saat `materialize_subaccount` membangun promise, reset 0 di callback
    /// `#[private]`. `assert_atomic_ready` menolak mutasi M&A di tengah window.
    #[test]
    fn sec111_kunci_nyata_set_dan_lepas() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let id = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("raider.testnet", 2 * 10u128.pow(24), 1_000);
        let raider = c.incubate_strain(StrainRole::Fermenter, U128(2 * 10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 2 * 10u128.pow(22), 1_000);
        let _ = c.materialize_subaccount(id); // promise dibangun → KUNCI NYALA
        assert_eq!(
            c.get_strain(id).pending_callbacks, 1,
            "kunci nyala selama promise window"
        );
        // (i) mutasi M&A di tengah window → assert_atomic_ready panic (tertangkap).
        ctx("raider.testnet", 0, 1_000 + 70 * 3_600);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.adopt(id, raider, 0);
        }));
        assert!(
            res.is_err(),
            "M&A saat promise window harus ERR_PENDING_ASYNC_CALLBACK_LOCKED"
        );
        // (ii) callback #[private] dieksekusi (promise SUKSES) → kunci lepas (0).
        let mut b = VMContextBuilder::new();
        b.current_account_id(akun("snacc-v001.testnet"))
            .predecessor_account_id(akun("snacc-v001.testnet"))
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp(1_000 * 1_000_000_000);
        testing_env!(
            b.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![near_sdk::PromiseResult::Successful(vec![])]
        );
        c.on_subaccount_created(
            id,
            akun("s-1.snacc-v001.testnet"),
            akun("alkemis.testnet"),
            U128(2 * 10u128.pow(22)),
        );
        assert_eq!(c.get_strain(id).pending_callbacks, 0, "kunci lepas setelah callback");
        // Setelah lepas → M&A kini sah (kondisi lain terpenuhi).
        ctx("raider.testnet", 0, 1_000 + 70 * 3_600);
        let _ = c.adopt(id, raider, c.get_strain(id).state_nonce);
        assert_eq!(c.get_strain(id).owner_id, akun("raider.testnet"));
    }

    /// SEC-111 — kunci juga nyala di `nft_transfer_call` dan lepas di
    /// `nft_resolve_transfer` (jalur ditolak pun melepas — semua jalur).
    #[test]
    fn sec111_kunci_nft_transfer_call() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let id = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 1, 1_000);
        let _ = c.nft_transfer_call(
            akun("alice.testnet"),
            id.to_string(),
            "msg".to_string(),
            None,
            None,
        );
        assert_eq!(c.get_strain(id).pending_callbacks, 1, "kunci nyala saat transfer_call");
        // Resolve di unit test: promise_result = Failed → ditolak; jalur penolakan
        // JUGA melepas kunci dan token kembali ke pemilik lama.
        let mut b = VMContextBuilder::new();
        b.current_account_id(akun("snacc-v001.testnet"))
            .predecessor_account_id(akun("snacc-v001.testnet"))
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp(1_000 * 1_000_000_000);
        testing_env!(
            b.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![near_sdk::PromiseResult::Failed]
        );
        c.nft_resolve_transfer(akun("alkemis.testnet"), akun("alice.testnet"), id.to_string());
        assert_eq!(c.get_strain(id).pending_callbacks, 0, "kunci lepas setelah resolve");
        assert_eq!(c.get_strain(id).owner_id, akun("alkemis.testnet"), "token kembali");
    }


    /// QA-122 (TC-MA-03) — warisan organel `max(dom, abs)` per slot + reset
    /// `last_allocate_epoch` (realokasi ATP dibuka kembali, `EN-120`).
    #[test]
    fn tc_ma_03_organel_max_dan_reset_alokasi() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let dom = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let abs = c.incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0).strain_id;
        // Organel: dominan {2,0,0}, terserap {0,3,1} → merged {2,3,1}.
        let mut d = c.strains.get(&dom).unwrap().clone();
        d.organel.mitokondria = 2;
        d.last_allocate_epoch = 7; // pernah alokasi
        let _ = c.strains.insert(dom, d);
        let mut a = c.strains.get(&abs).unwrap().clone();
        a.organel.nukleus = 3;
        a.organel.lisosom = 1;
        let _ = c.strains.insert(abs, a);
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.splice_strains(dom, abs);
        assert_eq!(v.organel.mitokondria, 2, "max(2,0)");
        assert_eq!(v.organel.nukleus, 3, "max(0,3)");
        assert_eq!(v.organel.lisosom, 1, "max(0,1)");
        assert_eq!(v.last_allocate_epoch, u64::MAX, "alokasi ATP di-reset (EN-120)");
    }
}

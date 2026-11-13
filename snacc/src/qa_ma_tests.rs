//! QA-120..122 (TC-MA-*) — M&A: `splice_strains` (role OR, a 2.5% tax, Gen+1,
//! ATP 5, inventory inheritance), `hostile_takeover_splice` (115%, 95/5, the `I-14` nonce,
//! ATP 10), organel inheritance `max(dom, abs)` + a `last_allocate_epoch` reset.
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so the test paths do not change
mod qa_ma_tests {
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

    /// QA-120 (TC-MA-01) — `splice_strains`: `role_mask |=`, a 2.5% tax to the treasury,
    /// `Gen+1`, the dominant's ATP −5, the absorbed inventory moves intact (conservation).
    #[test]
    fn tc_ma_01_splice_role_tax_atp_inventory() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let dom = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let abs = c.incubate_strain(StrainRole::Fermenter, U128(10u128.pow(24)), 0).strain_id;
        // Inventory on the absorbed cell: 7 spores + 2 enzymes — must move intact.
        let _ = c.inventories.insert((abs, "mt:spore-ignis".to_string()), 7);
        let _ = c.inventories.insert((abs, "mt:enzyme-amalgam".to_string()), 2);
        let treasury0 = c.treasury;
        let atp0 = c.get_strain(dom).atp_balance;
        ctx("alkemis.testnet", 0, 1_000);
        let v = c.splice_strains(dom, abs);
        // role OR; Gen+1; ATP −5.
        assert_eq!(v.role_mask, 0b011, "role_mask |= (I-08)");
        assert_eq!(v.generation, 1, "Gen+1");
        assert_eq!(v.atp_balance, atp0 - 5, "the splice ATP cost = 5 (§13a)");
        // A 2.5% tax of the absorbed balance (80% of the stake = 8×10^23 → tax 2×10^22).
        assert_eq!(c.treasury - treasury0, 2 * 10u128.pow(22), "a 2.5% tax (ADR-25)");
        // The inventory moves intact to the dominant.
        assert_eq!(c.get_inventory(dom, "mt:spore-ignis".to_string()).0, 7);
        assert_eq!(c.get_inventory(dom, "mt:enzyme-amalgam".to_string()).0, 2);
        assert_eq!(c.get_inventory(abs, "mt:spore-ignis".to_string()).0, 0);
        // The absorbed cell dies (apoptosis LC-07a).
        assert_eq!(c.get_strain(abs).status, crate::state::StrainStatus::SplicedBurned);
    }

    /// QA-121 (TC-MA-02) — hostile: pays `book × 115/100` (95% seller / 5% treasury),
    /// a stale `expected_target_nonce` → fails (`I-14`), the raider's ATP −10.
    #[test]
    fn tc_ma_02_hostile_115_nonce_atp() {
        ctx("owner.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let target = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("raider.testnet", 2 * 10u128.pow(24), 1_000);
        let raider = c.incubate_strain(StrainRole::Fermenter, U128(2 * 10u128.pow(24)), 0).strain_id;
        // Target Distressed: eff 1500 → 70 hours (the Extractor BMR 100).
        ctx("raider.testnet", 0, 1_000 + 70 * 3_600);
        assert_eq!(c.get_strain(target).effective_vitality, 1500);
        // A stale nonce → fails (I-14).
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.hostile_takeover_splice(raider, target, 99);
        }));
        assert!(res.is_err(), "a stale nonce must ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED");
        // The correct nonce (0) → takeover; book = 8×10^23; payment = 115% = 9.2×10^23.
        let treasury0 = c.treasury;
        let seller0 = c.ft_balance_of(acct("owner.testnet")).0;
        let atp0 = c.get_strain(raider).atp_balance;
        let (_, view) = c.hostile_takeover_splice(raider, target, 0);
        let payment = 92 * 10u128.pow(22); // 8×10^23 × 1.15
        assert_eq!(c.ft_balance_of(acct("owner.testnet")).0 - seller0, payment * 95 / 100, "95% to the seller");
        assert_eq!(c.treasury - treasury0, payment * 5 / 100, "5% to the treasury");
        assert_eq!(view.atp_balance, atp0 - 10, "the hostile ATP cost = 10 (§13a)");
        assert_eq!(c.get_strain(target).status, crate::state::StrainStatus::SplicedBurned);
    }

    /// SEC-111 (triage Option a — FIXED): `pending_callbacks` is now a REAL lock —
    /// set to 1 while `materialize_subaccount` builds the promise, reset to 0 in the
    /// `#[private]` callback. `assert_atomic_ready` rejects M&A mutations mid-window.
    #[test]
    fn sec111_real_lock_set_and_release() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let id = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("raider.testnet", 2 * 10u128.pow(24), 1_000);
        let raider = c.incubate_strain(StrainRole::Fermenter, U128(2 * 10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 2 * 10u128.pow(22), 1_000);
        let _ = c.materialize_subaccount(id); // the promise is built → the LOCK IS ON
        assert_eq!(
            c.get_strain(id).pending_callbacks, 1,
            "the lock is on during the promise window"
        );
        // (i) an M&A mutation mid-window → assert_atomic_ready panics (caught).
        ctx("raider.testnet", 0, 1_000 + 70 * 3_600);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.adopt(id, raider, 0);
        }));
        assert!(
            res.is_err(),
            "M&A during the promise window must ERR_PENDING_ASYNC_CALLBACK_LOCKED"
        );
        // (ii) the #[private] callback executes (the promise SUCCEEDS) → the lock is released (0).
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct("snacc-v001.testnet"))
            .predecessor_account_id(acct("snacc-v001.testnet"))
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
            acct("s-1.snacc-v001.testnet"),
            acct("alkemis.testnet"),
            U128(2 * 10u128.pow(22)),
        );
        assert_eq!(c.get_strain(id).pending_callbacks, 0, "the lock is released after the callback");
        // After release → M&A is now valid (the other conditions hold).
        ctx("raider.testnet", 0, 1_000 + 70 * 3_600);
        let _ = c.adopt(id, raider, c.get_strain(id).state_nonce);
        assert_eq!(c.get_strain(id).owner_id, acct("raider.testnet"));
    }

    /// SEC-111 — the lock is also on during `nft_transfer_call` and released in
    /// `nft_resolve_transfer` (the rejection path releases it too — all paths).
    #[test]
    fn sec111_lock_on_nft_transfer_call() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let id = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 1, 1_000);
        let _ = c.nft_transfer_call(
            acct("alice.testnet"),
            id.to_string(),
            None,
            None,
            "msg".to_string(),
        );
        assert_eq!(c.get_strain(id).pending_callbacks, 1, "the lock is on during transfer_call");
        // Resolved in the unit test: promise_result = Failed → rejected; the rejection path
        // ALSO releases the lock and the token returns to the old owner.
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct("snacc-v001.testnet"))
            .predecessor_account_id(acct("snacc-v001.testnet"))
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp(1_000 * 1_000_000_000);
        testing_env!(
            b.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![near_sdk::PromiseResult::Failed]
        );
        c.nft_resolve_transfer(acct("alkemis.testnet"), acct("alice.testnet"), id.to_string(), None);
        assert_eq!(c.get_strain(id).pending_callbacks, 0, "the lock is released after resolve");
        assert_eq!(c.get_strain(id).owner_id, acct("alkemis.testnet"), "the token returns");
    }


    /// QA-122 (TC-MA-03) — organel inheritance `max(dom, abs)` per slot + a reset
    /// of `last_allocate_epoch` (ATP reallocation reopens, `EN-120`).
    #[test]
    fn tc_ma_03_organelles_max_and_allocation_reset() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let dom = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0).strain_id;
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let abs = c.incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0).strain_id;
        // Organelles: dominant {2,0,0}, absorbed {0,3,1} → merged {2,3,1}.
        let mut d = c.strains.get(&dom).unwrap().clone();
        d.organel.mitokondria = 2;
        d.last_allocate_epoch = 7; // allocated before
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
        assert_eq!(v.last_allocate_epoch, u64::MAX, "the ATP allocation is reset (EN-120)");
    }
}

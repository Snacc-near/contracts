//! EN-126 — NEP-171 core `nft_transfer_call` / `nft_resolve_transfer`.
//! The transfer reuses the `nft_transfer` core (owner check, dead-cell rule, colony
//! index, `state_nonce`, `nep171` event); the receiver's answer decides a revert, which
//! only happens while the receiver still holds a live token.
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so test paths stay stable
mod qa_en126_tests {
    use crate::state::{StrainRole, StrainStatus};
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::mock::MockAction;
    use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
    use near_sdk::{testing_env, AccountId, Gas, NearToken, PromiseResult};

    const SNACC: &str = "snacc-v001.testnet";
    const OWNER: &str = "alkemis.testnet";
    const MARKET: &str = "market.testnet";
    const CAROL: &str = "carol.testnet";
    const PREPAID_TGAS: u64 = 300;

    fn acct(s: &str) -> AccountId {
        s.parse().unwrap()
    }

    fn ctx_gas(predecessor: &str, deposit_yocto: u128, prepaid_tgas: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(SNACC))
            .signer_account_id(acct(predecessor))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .prepaid_gas(Gas::from_tgas(prepaid_tgas))
            .block_timestamp(1_000 * 1_000_000_000);
        testing_env!(b.build());
    }

    fn ctx(predecessor: &str, deposit_yocto: u128) {
        ctx_gas(predecessor, deposit_yocto, PREPAID_TGAS);
    }

    /// The `nft_resolve_transfer` callback context: called by the contract itself with
    /// the receiver's `nft_on_transfer` outcome as promise result 0.
    fn callback_ctx(result: PromiseResult) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(SNACC))
            .signer_account_id(acct(OWNER))
            .predecessor_account_id(acct(SNACC))
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp(1_000 * 1_000_000_000);
        testing_env!(
            b.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![result]
        );
    }

    fn setup() -> (SnaccContract, u64) {
        ctx(OWNER, 10u128.pow(24));
        let mut c = SnaccContract::new();
        let id = c
            .incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0)
            .strain_id;
        (c, id)
    }

    /// `OWNER` lists strain `id` at `MARKET` via `nft_transfer_call`.
    fn transfer_call_to_market(c: &mut SnaccContract, id: u64) {
        ctx(OWNER, 1);
        let _ = c.nft_transfer_call(
            acct(MARKET),
            id.to_string(),
            None,
            Some("list".to_string()),
            r#"{"price":"1000"}"#.to_string(),
        );
    }

    fn resolve(c: &mut SnaccContract, id: u64, result: PromiseResult) -> bool {
        callback_ctx(result);
        c.nft_resolve_transfer(acct(OWNER), acct(MARKET), id.to_string(), None)
    }

    fn ok_json(v: &str) -> PromiseResult {
        PromiseResult::Successful(v.as_bytes().to_vec())
    }

    fn transfer_events(logs: &[String]) -> Vec<String> {
        logs.iter()
            .filter(|l| l.contains(r#""standard":"nep171""#) && l.contains(r#""event":"nft_transfer""#))
            .cloned()
            .collect()
    }

    /// TC-EN-126-01 — success path: ownership + colony index move at call time, the lock
    /// is on, `nft_on_transfer(sender_id, previous_owner_id, token_id, msg)` is scheduled
    /// on the receiver with the gas reservation, and the resolve (`false` from the
    /// receiver) keeps the transfer and returns `true`.
    #[test]
    fn tc_en_126_01_success_receiver_keeps_token() {
        let (mut c, id) = setup();
        let nonce0 = c.get_strain(id).state_nonce;
        transfer_call_to_market(&mut c, id);
        let v = c.get_strain(id);
        assert_eq!(v.owner_id, acct(MARKET));
        assert_eq!(v.pending_callbacks, 1, "SEC-111 lock on during the window");
        assert!(v.state_nonce > nonce0, "state_nonce bumped by the transfer");
        assert_eq!(c.get_colony(acct(OWNER)).len(), 0);
        assert_eq!(c.get_colony(acct(MARKET)).len(), 1);
        let ev = transfer_events(&get_logs());
        assert_eq!(ev.len(), 1);
        assert!(ev[0].contains(r#""old_owner_id":"alkemis.testnet""#));
        assert!(ev[0].contains(r#""new_owner_id":"market.testnet""#));
        assert!(ev[0].contains(&format!(r#""token_ids":["{}"]"#, id)));
        assert!(ev[0].contains(r#""memo":"list""#));

        let receipts = get_created_receipts();
        let on_transfer = receipts
            .iter()
            .find(|r| r.receiver_id == acct(MARKET))
            .expect("receipt to the receiver");
        match &on_transfer.actions[0] {
            MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                prepaid_gas,
                ..
            } => {
                assert_eq!(method_name, b"nft_on_transfer");
                assert_eq!(*attached_deposit, NearToken::from_yoctonear(0));
                assert_eq!(*prepaid_gas, Gas::from_tgas(PREPAID_TGAS - 50));
                let a: near_sdk::serde_json::Value = near_sdk::serde_json::from_slice(args).unwrap();
                assert_eq!(a["sender_id"], OWNER);
                assert_eq!(a["previous_owner_id"], OWNER);
                assert_eq!(a["token_id"], id.to_string());
                assert_eq!(a["msg"], r#"{"price":"1000"}"#);
            }
            other => panic!("unexpected action {other:?}"),
        }
        let resolve_r = receipts
            .iter()
            .find(|r| r.receiver_id == acct(SNACC))
            .expect("resolve callback receipt");
        match &resolve_r.actions[0] {
            MockAction::FunctionCallWeight {
                method_name,
                prepaid_gas,
                args,
                ..
            } => {
                assert_eq!(method_name, b"nft_resolve_transfer");
                assert!(*prepaid_gas >= Gas::from_tgas(20));
                let a: near_sdk::serde_json::Value = near_sdk::serde_json::from_slice(args).unwrap();
                assert_eq!(a["previous_owner_id"], OWNER);
                assert_eq!(a["receiver_id"], MARKET);
                assert_eq!(a["token_id"], id.to_string());
                assert!(a["approved_account_ids"].is_null());
            }
            other => panic!("unexpected action {other:?}"),
        }

        assert!(resolve(&mut c, id, ok_json("false")), "transfer stands → true");
        let v = c.get_strain(id);
        assert_eq!(v.owner_id, acct(MARKET));
        assert_eq!(v.pending_callbacks, 0, "lock released");
        assert!(transfer_events(&get_logs()).is_empty(), "no revert event");
    }

    /// TC-EN-126-02 — the receiver returns `true` → the token goes back to the previous
    /// owner, the colony index recovers, a revert `nft_transfer` event, `false`.
    #[test]
    fn tc_en_126_02_receiver_returns_true_reverted() {
        let (mut c, id) = setup();
        transfer_call_to_market(&mut c, id);
        assert!(!resolve(&mut c, id, ok_json("true")));
        let v = c.get_strain(id);
        assert_eq!(v.owner_id, acct(OWNER));
        assert_eq!(v.pending_callbacks, 0);
        assert_eq!(c.get_colony(acct(OWNER)).len(), 1);
        assert_eq!(c.get_colony(acct(MARKET)).len(), 0);
        let ev = transfer_events(&get_logs());
        assert_eq!(ev.len(), 1);
        assert!(ev[0].contains(r#""old_owner_id":"market.testnet""#));
        assert!(ev[0].contains(r#""new_owner_id":"alkemis.testnet""#));
    }

    /// TC-EN-126-03 — the receiver panics (failed promise) → reverted.
    #[test]
    fn tc_en_126_03_receiver_panics_reverted() {
        let (mut c, id) = setup();
        transfer_call_to_market(&mut c, id);
        assert!(!resolve(&mut c, id, PromiseResult::Failed));
        assert_eq!(c.get_strain(id).owner_id, acct(OWNER));
        assert_eq!(c.get_strain(id).pending_callbacks, 0);
        assert_eq!(transfer_events(&get_logs()).len(), 1);
    }

    /// TC-EN-126-04 — the receiver already moved the token on (to `CAROL`) and then
    /// answers `true` → NOT reverted (the third party keeps it), `true`, no event.
    #[test]
    fn tc_en_126_04_receiver_moved_token_not_reverted() {
        let (mut c, id) = setup();
        transfer_call_to_market(&mut c, id);
        ctx(MARKET, 1);
        c.nft_transfer(acct(CAROL), id.to_string(), None, None);
        assert!(resolve(&mut c, id, ok_json("true")));
        let v = c.get_strain(id);
        assert_eq!(v.owner_id, acct(CAROL));
        assert_eq!(v.pending_callbacks, 0);
        assert_eq!(c.get_colony(acct(CAROL)).len(), 1);
        assert_eq!(c.get_colony(acct(MARKET)).len(), 0);
        assert_eq!(c.get_colony(acct(OWNER)).len(), 0);
        assert!(transfer_events(&get_logs()).is_empty());
    }

    /// TC-EN-126-05 — no attached yoctoNEAR → `ERR_YOCTO_REQUIRED`.
    #[test]
    #[should_panic(expected = "ERR_YOCTO_REQUIRED")]
    fn tc_en_126_05_missing_yocto_rejected() {
        let (mut c, id) = setup();
        ctx(OWNER, 0);
        let _ = c.nft_transfer_call(acct(MARKET), id.to_string(), None, None, String::new());
    }

    /// TC-EN-126-05b — more than 1 yoctoNEAR is also rejected (exactly 1, like `nft_transfer`).
    #[test]
    #[should_panic(expected = "ERR_YOCTO_REQUIRED")]
    fn tc_en_126_05b_two_yocto_rejected() {
        let (mut c, id) = setup();
        ctx(OWNER, 2);
        let _ = c.nft_transfer_call(acct(MARKET), id.to_string(), None, None, String::new());
    }

    /// TC-EN-126-06 — a non-owner cannot `nft_transfer_call` → `ERR_NOT_OWNED_BY_CALLEE`.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn tc_en_126_06_non_owner_rejected() {
        let (mut c, id) = setup();
        ctx(CAROL, 1);
        let _ = c.nft_transfer_call(acct(MARKET), id.to_string(), None, None, String::new());
    }

    /// TC-EN-126-07 — a non-`bool` answer and an oversized answer both count as "give back".
    #[test]
    fn tc_en_126_07_non_bool_or_oversized_result_reverted() {
        let (mut c, id) = setup();
        transfer_call_to_market(&mut c, id);
        assert!(!resolve(&mut c, id, ok_json(r#""yes""#)));
        assert_eq!(c.get_strain(id).owner_id, acct(OWNER));

        transfer_call_to_market(&mut c, id);
        let long = format!("false{}", " ".repeat(64));
        assert!(!resolve(&mut c, id, ok_json(&long)));
        assert_eq!(c.get_strain(id).owner_id, acct(OWNER));
        assert_eq!(c.get_strain(id).pending_callbacks, 0);
    }

    /// TC-EN-126-08 — a cell that died during the window is not moved back (dead cells
    /// never move, same rule as `nft_transfer`); the lock is still released.
    #[test]
    fn tc_en_126_08_dead_cell_not_reverted() {
        let (mut c, id) = setup();
        transfer_call_to_market(&mut c, id);
        let mut v = c.strains.get(&id).unwrap().clone();
        v.status = StrainStatus::Liquidated;
        let _ = c.strains.insert(id, v);
        assert!(resolve(&mut c, id, ok_json("true")));
        assert_eq!(c.get_strain(id).owner_id, acct(MARKET));
        assert_eq!(c.get_strain(id).pending_callbacks, 0);
    }

    /// TC-EN-126-09 — prepaid gas at or below the reservation → `ERR_MORE_GAS_REQUIRED`.
    #[test]
    #[should_panic(expected = "ERR_MORE_GAS_REQUIRED")]
    fn tc_en_126_09_not_enough_gas_rejected() {
        let (mut c, id) = setup();
        ctx_gas(OWNER, 1, 50);
        let _ = c.nft_transfer_call(acct(MARKET), id.to_string(), None, None, String::new());
    }

    /// TC-EN-126-10 — dead cells cannot be sent with `nft_transfer_call` either.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn tc_en_126_10_dead_cell_transfer_call_rejected() {
        let (mut c, id) = setup();
        let mut v = c.strains.get(&id).unwrap().clone();
        v.status = StrainStatus::Liquidated;
        let _ = c.strains.insert(id, v);
        ctx(OWNER, 1);
        let _ = c.nft_transfer_call(acct(MARKET), id.to_string(), None, None, String::new());
    }

    /// TC-EN-126-11 — a self `nft_transfer_call` is rejected like `nft_transfer`.
    #[test]
    #[should_panic(expected = "ERR_SAME_STRAIN")]
    fn tc_en_126_11_self_transfer_call_rejected() {
        let (mut c, id) = setup();
        ctx(OWNER, 1);
        let _ = c.nft_transfer_call(acct(OWNER), id.to_string(), None, None, String::new());
    }

    /// TC-EN-126-12 — the receiver forwards with its own `nft_transfer_call`: the lock is a
    /// counter (2 → 1 → 0), so the inner resolve does not unlock the outer window.
    #[test]
    fn tc_en_126_12_nested_transfer_call_lock_counts() {
        let (mut c, id) = setup();
        transfer_call_to_market(&mut c, id);
        ctx(MARKET, 1);
        let _ = c.nft_transfer_call(acct(CAROL), id.to_string(), None, None, String::new());
        assert_eq!(c.get_strain(id).pending_callbacks, 2);
        // Inner resolve: CAROL keeps it.
        callback_ctx(ok_json("false"));
        assert!(c.nft_resolve_transfer(acct(MARKET), acct(CAROL), id.to_string(), None));
        assert_eq!(c.get_strain(id).pending_callbacks, 1);
        // Outer resolve: MARKET said "give back", but the token is with CAROL now.
        assert!(resolve(&mut c, id, ok_json("true")));
        assert_eq!(c.get_strain(id).owner_id, acct(CAROL));
        assert_eq!(c.get_strain(id).pending_callbacks, 0);
    }

    /// TC-EN-126-13 — `approval_id` is a NEP-171 number (`Option<u64>`) and is ignored.
    #[test]
    fn tc_en_126_13_numeric_approval_id_accepted() {
        let (mut c, id) = setup();
        ctx(OWNER, 1);
        c.nft_transfer(acct(CAROL), id.to_string(), Some(7), None);
        assert_eq!(c.get_strain(id).owner_id, acct(CAROL));
        ctx(CAROL, 1);
        let _ = c.nft_transfer_call(acct(MARKET), id.to_string(), Some(3), None, String::new());
        assert_eq!(c.get_strain(id).owner_id, acct(MARKET));
    }
}

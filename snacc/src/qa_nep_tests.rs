//! SEC-103 — NEP compliance, ported from `reports/nep-validation.md`:
//! - NEP-177: `nft_metadata` (exact standard shape, data-URI SVG icon).
//! - NEP-145: `storage_deposit(registration_only)` + `storage_balance_bounds` +
//!   `storage_unregister(force)` — the deposit/withdraw/unregister cycle.
//! - NEP-181: `nft_total_supply` / `nft_tokens` / `nft_supply_for_owner` /
//!   `nft_tokens_for_owner` (exact scan of the `strains` map).
//!
//! All views are read-only; no behavior change on the legacy economic paths.
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so test paths stay stable
mod qa_nep_tests {
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

    // ── NEP-177 ─────────────────────────────────────────────────────────────

    /// TC-NEP-01 — `nft_metadata` matches the standard exactly: spec `nft-1.0.0`,
    /// name `Snacc`, symbol `$SNACC`, a base64 SVG data-URI icon.
    #[test]
    fn tc_nep01_nft_metadata_exact_shape() {
        ctx("alkemis.testnet", 0, 1_000);
        let c = SnaccContract::new();
        let m = c.nft_metadata();
        assert_eq!(m.spec, "nft-1.0.0", "NEP-177 spec must be nft-1.0.0");
        assert_eq!(m.name, "Snacc");
        assert_eq!(m.symbol, "$SNACC");
        let icon = m.icon.expect("icon required (SVG data-URI)");
        assert!(
            icon.starts_with("data:image/svg+xml;base64,"),
            "icon must be an SVG data-URI: {icon}"
        );
        // Same icon as ft_metadata (one visual identity).
        assert_eq!(c.ft_metadata().icon, Some(icon.clone()), "FT/NFT icon consistent");
    }

    /// TC-NEP-02 — `nft_token` keeps the token view intact: token_id,
    /// owner_id, media data-URI SVG (token-level NEP-177 still holds).
    #[test]
    fn tc_nep02_nft_metadata_present_on_nft_token() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let id = c
            .incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0)
            .strain_id;
        ctx("alkemis.testnet", 0, 1_000);
        let m = c.nft_metadata();
        assert_eq!(m.spec, "nft-1.0.0");
        let t = c.nft_token(id.to_string());
        assert_eq!(t.token_id, id.to_string());
        assert_eq!(t.owner_id, test_account("alkemis.testnet"));
        assert!(
            t.metadata.media.starts_with("data:image/svg+xml;base64,"),
            "token media = SVG data-URI (F7)"
        );
        assert_eq!(t.metadata.title, format!("Snacc Strain #{}", id));
    }

    // ── NEP-145 ─────────────────────────────────────────────────────────────

    /// TC-NEP-03 — `storage_balance_bounds` is sane: min = 5 milliNEAR, max = None.
    #[test]
    fn tc_nep03_storage_balance_bounds() {
        ctx("alkemis.testnet", 0, 1_000);
        let c = SnaccContract::new();
        let b = c.storage_balance_bounds();
        assert_eq!(
            b.min.0,
            NearToken::from_millinear(5).as_yoctonear(),
            "min = the recorded STORAGE_DEPOSIT_MILLINEAR=5 decision"
        );
        assert_eq!(b.max, None, "no upper bound");
    }

    /// TC-NEP-04 — full cycle: deposit → withdraw → unregister (refund).
    #[test]
    fn tc_nep04_storage_cycle_deposit_withdraw_unregister() {
        ctx("alkemis.testnet", 0, 1_000);
        let mut c = SnaccContract::new();
        let min = NearToken::from_millinear(5).as_yoctonear();
        // Deposit twice the minimum.
        ctx("alkemis.testnet", 2 * min, 1_000);
        let sb = c.storage_deposit(None, None);
        assert_eq!(sb.total.0, 2 * min);
        assert_eq!(
            c.storage_balance_of(test_account("alkemis.testnet")).unwrap().total.0,
            2 * min
        );
        // Withdraw the excess (no `$SNACC` balance → nothing locked; 1 yoctoNEAR guard).
        ctx("alkemis.testnet", 1, 1_000);
        let sw = c.storage_withdraw(Some(U128(min)));
        assert_eq!(sw.total.0, min);
        assert_eq!(
            c.storage_balance_of(test_account("alkemis.testnet")).unwrap().total.0,
            min
        );
        // Unregister → refund the rest, the entry disappears, returns Some(balance) (1 yocto).
        ctx("alkemis.testnet", 1, 1_000);
        let un = c.storage_unregister(None).expect("Some while registered");
        assert_eq!(un.total.0, min);
        assert_eq!(
            c.storage_balance_of(test_account("alkemis.testnet")),
            None,
            "registration gone after unregister"
        );
        // Unregister again → None (NEP-145 semantics).
        ctx("alkemis.testnet", 1, 1_000);
        assert_eq!(c.storage_unregister(None), None, "not registered → None");
    }

    /// TC-NEP-05 — `registration_only=true`: registers only; the excess is NOT
    /// credited (total = min), an already-registered account stays registered (full refund).
    #[test]
    fn tc_nep05_storage_deposit_registration_only() {
        ctx("alkemis.testnet", 0, 1_000);
        let mut c = SnaccContract::new();
        let min = NearToken::from_millinear(5).as_yoctonear();
        // New registration with a 2×min deposit → only min is credited (rest refunded).
        ctx("alkemis.testnet", 2 * min, 1_000);
        let sb = c.storage_deposit(None, Some(true));
        assert_eq!(sb.total.0, min, "registration_only: only min credited");
        assert_eq!(
            c.storage_balance_of(test_account("alkemis.testnet")).unwrap().total.0,
            min
        );
        // Already registered + registration_only + more deposit → total unchanged.
        ctx("alkemis.testnet", min, 1_000);
        let sb2 = c.storage_deposit(None, Some(true));
        assert_eq!(sb2.total.0, min, "no balance added (full refund)");
        // Below the minimum for a NEW registration → rejected.
        ctx("bob.testnet", min - 1, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.storage_deposit(None, Some(true));
        }));
        assert!(res.is_err(), "registration_only below min must be rejected");
    }

    // ── NEP-181 ─────────────────────────────────────────────────────────────

    /// TC-NEP-06 — `nft_total_supply` + `nft_tokens` pagination (offset + limit).
    #[test]
    fn tc_nep06_nft_total_supply_and_tokens() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        for _ in 0..4 {
            ctx("alkemis.testnet", 10u128.pow(24), 1_000);
            let _ = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0);
        }
        ctx("alkemis.testnet", 0, 1_000);
        assert_eq!(c.nft_total_supply().0, 4, "4 tokens minted");
        // Page 1: offset 0, limit 2 → tokens 1, 2.
        let page1 = c.nft_tokens(Some(U128(0)), Some(2));
        assert_eq!(
            page1.iter().map(|t| t.token_id.clone()).collect::<Vec<_>>(),
            vec!["1".to_string(), "2".to_string()]
        );
        // Page 2: offset 2, limit 2 → tokens 3, 4.
        let page2 = c.nft_tokens(Some(U128(2)), Some(2));
        assert_eq!(
            page2.iter().map(|t| t.token_id.clone()).collect::<Vec<_>>(),
            vec!["3".to_string(), "4".to_string()]
        );
        // No offset/limit → everything.
        let all = c.nft_tokens(None, None);
        assert_eq!(all.len(), 4);
        // Consistent with `nft_token` (one source of truth).
        assert_eq!(c.nft_token("3".to_string()).metadata, page2[0].metadata);
    }

    /// TC-NEP-07 — `nft_supply_for_owner` exact: two owners, and an `nft_transfer`
    /// moves the supply between them.
    #[test]
    fn tc_nep07_nft_supply_for_owner() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let a = c
            .incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0)
            .strain_id;
        ctx("bob.testnet", 10u128.pow(24), 1_000);
        let _b = c
            .incubate_strain(StrainRole::Fermenter, U128(10u128.pow(24)), 0)
            .strain_id;
        ctx("alkemis.testnet", 0, 1_000);
        assert_eq!(c.nft_supply_for_owner(test_account("alkemis.testnet")).0, 1);
        assert_eq!(c.nft_supply_for_owner(test_account("bob.testnet")).0, 1);
        // Transfer a → bob (dummy approval_id/memo — NEP-178 approvals not active; 1 yocto).
        ctx("alkemis.testnet", 1, 1_000);
        c.nft_transfer(
            test_account("bob.testnet"),
            a.to_string(),
            Some("0".to_string()),
            None,
        );
        assert_eq!(c.nft_supply_for_owner(test_account("alkemis.testnet")).0, 0);
        assert_eq!(c.nft_supply_for_owner(test_account("bob.testnet")).0, 2);
        assert_eq!(c.nft_supply_for_owner(test_account("takada.testnet")).0, 0);
    }

    /// TC-NEP-08 — `nft_tokens_for_owner` + pagination; a `divide_strain` child
    /// (which is NOT pushed into the `strains_by_owner` index) is still counted
    /// (the view scans `strains` exactly).
    #[test]
    fn tc_nep08_nft_tokens_for_owner_exact_scan() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let _ = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 0);
        let _ = c.incubate_strain(StrainRole::Fermenter, U128(10u128.pow(24)), 0);
        // bob owns 1 strain.
        ctx("bob.testnet", 10u128.pow(24), 1_000);
        let _ = c.incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0);
        // alkemis: feed + splice 1<2 + advance_epoch + divide → child id 4 (NOT in
        // the `strains_by_owner` index — exactly what this exact-scan test proves).
        ctx("alkemis.testnet", 0, 1_000);
        let _ = c.feed_strain(1, U128(10u128.pow(18)));
        ctx("alkemis.testnet", 0, 1_000);
        let _ = c.splice_strains(1, 2);
        ctx("alkemis.testnet", 0, 1_000);
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        ctx("alkemis.testnet", 0, 1_000);
        let _ = c.divide_strain(1);
        ctx("alkemis.testnet", 0, 1_000);
        // Exact: alkemis owns 3 tokens (1, spliced 2, child 4); bob owns 1 (3).
        assert_eq!(c.nft_supply_for_owner(test_account("alkemis.testnet")).0, 3);
        let owned = c.nft_tokens_for_owner(test_account("alkemis.testnet"), None, None);
        assert_eq!(
            owned.iter().map(|t| t.token_id.clone()).collect::<Vec<_>>(),
            vec!["1".to_string(), "2".to_string(), "4".to_string()],
            "ascending by token_id"
        );
        assert_eq!(c.nft_supply_for_owner(test_account("bob.testnet")).0, 1);
        // Pagination: offset 1 limit 1 → only token "2".
        let page = c.nft_tokens_for_owner(test_account("alkemis.testnet"), Some(U128(1)), Some(1));
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].token_id, "2");
        assert_eq!(page[0].owner_id, test_account("alkemis.testnet"));
    }
}
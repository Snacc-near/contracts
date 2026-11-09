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

    // ── NEP-141 `ft_mint` (N141-105) ───────────────────────────────────────
    // NOTE: `near_sdk::test_utils::get_logs()` only returns the logs of the
    // CURRENT mock context (each `ctx()` starts a fresh one), so the tests
    // drain the logs into an accumulator after every mutating call.

    fn drain_logs(acc: &mut Vec<String>) {
        acc.extend(near_sdk::test_utils::get_logs());
    }

    fn count_in(logs: &[String], event: &str) -> usize {
        let marker = format!(r#""event":"{}""#, event);
        logs.iter().filter(|l| l.contains(marker.as_str())).count()
    }

    /// Sum the `amount` of every `EVENT_JSON` line with `"event":"<event>"`.
    /// All NEP-141 events use the string form (`"amount":"123"`); the parser
    /// still tolerates a bare number for robustness against old logs.
    fn sum_in(logs: &[String], event: &str) -> u128 {
        let marker = format!(r#""event":"{}""#, event);
        let mut total = 0u128;
        for line in logs {
            if !line.contains(marker.as_str()) {
                continue;
            }
            let key = r#""amount":"#;
            let start = line.find(key).expect("amount field present") + key.len();
            let rest = &line[start..];
            // String form (`"amount":"123"`) carries the closing quote; the
            // number form (`"amount":123`) does not.
            let digits = rest.strip_prefix('"').unwrap_or(rest);
            let end = digits
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(digits.len());
            total += digits[..end].parse::<u128>().unwrap_or(0);
        }
        total
    }

    /// TC-NEP-09 — incubating with an attached NEAR stake emits exactly one
    /// NEP-141 `ft_mint` (standard `nep141`, amount as string) for the full
    /// stake, and the supply grows by the same amount.
    #[test]
    fn tc_nep09_ft_mint_event_on_near_stake_incubate() {
        let stake: u128 = 10u128.pow(24);
        ctx("alkemis.testnet", stake, 1_000);
        let mut c = SnaccContract::new();
        drain_logs(&mut Vec::new());
        ctx("alkemis.testnet", stake, 1_000);
        let _ = c.incubate_strain(StrainRole::Extractor, U128(stake), 0);

        assert_eq!(
            c.ft_total_supply().0, stake,
            "the NEAR stake is born into the supply"
        );
        let mut all = Vec::new();
        drain_logs(&mut all);
        assert_eq!(count_in(&all, "ft_mint"), 1, "exactly one ft_mint");
        let expected = format!(
            r#"EVENT_JSON:{{"standard":"nep141","version":"1.0.0","event":"ft_mint","data":[{{"owner_id":"alkemis.testnet","amount":"{}","memo":"incubate_strain"}}]}}"#,
            stake
        );
        assert!(
            all.iter().any(|l| l == &expected),
            "exact ft_mint line missing:\n{expected}\nlogs: {:?}",
            all
        );
    }

    /// TC-NEP-10 — a balance-funded incubate (`attached = 0`, the stake is
    /// debited from the caller's `account_balances`) mints nothing: no
    /// `ft_mint`, no supply change. The lysis bounty that funded the account
    /// is likewise a pure transfer (strain balance → caller + treasury).
    #[test]
    fn tc_nep10_no_ft_mint_on_balance_funded_incubate() {
        let stake: u128 = 10_000;
        let mut all = Vec::new();
        ctx("alkemis.testnet", stake, 1_000);
        let mut c = SnaccContract::new();
        ctx("alkemis.testnet", stake, 1_000);
        let _s1 = c.incubate_strain(StrainRole::Extractor, U128(stake), 0).strain_id;
        drain_logs(&mut all);
        ctx("bob.testnet", stake, 1_000);
        let s2 = c.incubate_strain(StrainRole::Extractor, U128(stake), 0).strain_id;
        drain_logs(&mut all);
        assert_eq!(count_in(&all, "ft_mint"), 2);
        let supply_after_mints = c.ft_total_supply().0;
        assert_eq!(supply_after_mints, 2 * stake);

        // Lysis bounty (10% of the starved strain balance) funds alkemis's
        // account — a transfer, not a mint.
        ctx("alkemis.testnet", 0, 1_000 + 90 * 3600);
        let (bounty, _) = c.harvest_starving_strain(s2);
        drain_logs(&mut all);
        assert!(bounty.0 > 0, "the bounty must fund the account");
        assert_eq!(c.ft_total_supply().0, supply_after_mints, "no mint on lysis");
        assert_eq!(count_in(&all, "ft_mint"), 2, "no ft_mint on lysis bounty");

        // Balance-funded incubate: the account is debited, supply untouched.
        let funded = bounty.0 / 2;
        ctx("alkemis.testnet", 0, 1_000);
        let _ = c.incubate_strain(StrainRole::Extractor, U128(funded), 0);
        drain_logs(&mut all);
        assert_eq!(
            c.ft_total_supply().0, supply_after_mints,
            "balance-funded incubate grows no supply"
        );
        assert_eq!(count_in(&all, "ft_mint"), 2, "no ft_mint on balance-funded incubate");
        assert_eq!(
            c.ft_balance_of(test_account("alkemis.testnet")).0,
            bounty.0 - funded,
            "the account balance paid the stake"
        );
    }

    /// TC-NEP-11 — `ft_transfer` emits `ft_transfer` only: no `ft_mint`, no
    /// supply change (sender debited, receiver credited, same total).
    #[test]
    fn tc_nep11_no_ft_mint_on_ft_transfer() {
        let stake: u128 = 10_000;
        let mut all = Vec::new();
        ctx("alkemis.testnet", stake, 1_000);
        let mut c = SnaccContract::new();
        ctx("alkemis.testnet", stake, 1_000);
        let _s1 = c.incubate_strain(StrainRole::Extractor, U128(stake), 0).strain_id;
        drain_logs(&mut all);
        ctx("bob.testnet", stake, 1_000);
        let s2 = c.incubate_strain(StrainRole::Extractor, U128(stake), 0).strain_id;
        drain_logs(&mut all);
        // Register bob (NEP-145 — NEAR storage, no `$SNACC` ledger impact).
        let min = NearToken::from_millinear(5).as_yoctonear();
        ctx("bob.testnet", min, 1_000);
        let _ = c.storage_deposit(None, None);
        drain_logs(&mut all);
        // Fund alkemis's account via the lysis bounty, then transfer 100 to bob.
        ctx("alkemis.testnet", 0, 1_000 + 90 * 3600);
        let (bounty, _) = c.harvest_starving_strain(s2);
        drain_logs(&mut all);
        assert!(bounty.0 > 100);

        let mints_before = count_in(&all, "ft_mint");
        let supply_before = c.ft_total_supply().0;
        ctx("alkemis.testnet", 1, 1_000);
        c.ft_transfer(test_account("bob.testnet"), U128(100), None);
        drain_logs(&mut all);

        assert_eq!(count_in(&all, "ft_mint"), mints_before, "no ft_mint on transfer");
        assert_eq!(count_in(&all, "ft_transfer"), 1, "ft_transfer emitted");
        assert_eq!(
            c.ft_total_supply().0, supply_before,
            "a pure transfer changes no supply"
        );
        assert_eq!(c.ft_balance_of(test_account("bob.testnet")).0, 100);
        assert_eq!(
            c.ft_balance_of(test_account("alkemis.testnet")).0,
            bounty.0 - 100
        );
    }

    /// TC-NEP-12 — `I-01` consistency over a multi-step scenario:
    /// Σ(`ft_mint`) − Σ(`ft_burn`) == Δ`ft_total_supply`. The scenario mixes
    /// NEAR-stake incubates (mints), a Hypothermia rebate (treasury → strain
    /// transfer), a lysis bounty (transfer), a balance-funded incubate (no
    /// mint), an `ft_burn`, and an `ft_transfer` — only the two mint paths
    /// and the burn move the net event balance.
    #[test]
    fn tc_nep12_ft_mint_minus_ft_burn_equals_supply_delta() {
        let stake_a: u128 = 20_000;
        let stake_b: u128 = 10_000;
        let mut all = Vec::new();
        ctx("alkemis.testnet", stake_a, 1_000);
        let mut c = SnaccContract::new();
        ctx("alkemis.testnet", stake_a, 1_000);
        let s1 = c.incubate_strain(StrainRole::Extractor, U128(stake_a), 0).strain_id;
        drain_logs(&mut all);
        ctx("bob.testnet", stake_b, 1_000);
        let s2 = c.incubate_strain(StrainRole::Extractor, U128(stake_b), 0).strain_id;
        drain_logs(&mut all);

        // Hypothermia rebate (treasury → strain): a transfer, no `ft_mint`.
        // fee = 3_000 on supply 30_000 → temp = 100 < 600.
        let fee: u128 = 3_000;
        let strain_bal_before = c.snacc_balances.get(&s1).copied().unwrap_or(0);
        ctx("alkemis.testnet", fee, 1_000);
        let _ = c.extract_spores(s1, "mt:spore-ignis".to_string(), U128(fee));
        drain_logs(&mut all);
        assert!(
            c.snacc_balances.get(&s1).copied().unwrap_or(0) > strain_bal_before,
            "the rebate must credit the strain"
        );
        assert_eq!(count_in(&all, "ft_mint"), 2, "no ft_mint on the rebate");

        // Register bob, then fund alkemis's account by harvesting s2.
        let min = NearToken::from_millinear(5).as_yoctonear();
        ctx("bob.testnet", min, 1_000);
        let _ = c.storage_deposit(None, None);
        drain_logs(&mut all);
        ctx("alkemis.testnet", 0, 1_000 + 90 * 3600);
        let (bounty, _) = c.harvest_starving_strain(s2);
        drain_logs(&mut all);
        assert!(bounty.0 > 0);

        // Balance-funded incubate (no mint), then ft_burn + ft_transfer.
        ctx("alkemis.testnet", 0, 1_000);
        let funded = bounty.0 / 2;
        let _ = c.incubate_strain(StrainRole::Extractor, U128(funded), 0);
        drain_logs(&mut all);
        ctx("alkemis.testnet", 0, 1_000);
        c.ft_burn(s1, U128(500));
        drain_logs(&mut all);
        ctx("alkemis.testnet", 1, 1_000);
        c.ft_transfer(test_account("bob.testnet"), U128(100), None);
        drain_logs(&mut all);

        let minted = sum_in(&all, "ft_mint");
        let burned = sum_in(&all, "ft_burn");
        let supply = c.ft_total_supply().0;
        assert_eq!(minted, stake_a + stake_b, "only the two NEAR stakes minted");
        assert_eq!(burned, 500);
        assert_eq!(
            minted - burned,
            supply,
            "I-01: the event net must equal the supply"
        );
        assert_eq!(supply, stake_a + stake_b - 500);
    }

    /// TC-NEP-13 — N141-105: `ft_transfer` and `ft_burn` MUST encode `amount`
    /// as a decimal string (NEP-141), `memo` as a JSON string or `null`, and
    /// a value above 2^53 must survive string round-trip losslessly.
    #[test]
    fn tc_nep13_ft_transfer_and_ft_burn_amount_is_string() {
        let stake_a: u128 = 10u128.pow(18);
        let big: u128 = 9_007_199_254_740_993; // 2^53 + 1, not representable as f64
        let mut all = Vec::new();
        ctx("alkemis.testnet", stake_a, 1_000);
        let mut c = SnaccContract::new();
        ctx("alkemis.testnet", stake_a, 1_000);
        let s1 = c.incubate_strain(StrainRole::Extractor, U128(stake_a), 0).strain_id;
        drain_logs(&mut all);
        ctx("alkemis.testnet", stake_a, 1_000);
        let s2 = c.incubate_strain(StrainRole::Extractor, U128(stake_a), 0).strain_id;
        drain_logs(&mut all);

        // Register bob, then fund alkemis's account by harvesting s1 (its
        // balance is zeroed, so the burn below uses s2).
        let min = NearToken::from_millinear(5).as_yoctonear();
        ctx("bob.testnet", min, 1_000);
        let _ = c.storage_deposit(None, None);
        drain_logs(&mut all);
        ctx("alkemis.testnet", 0, 1_000 + 90 * 3600);
        let (bounty, _) = c.harvest_starving_strain(s1);
        drain_logs(&mut all);
        assert!(bounty.0 > big, "the bounty must cover the big transfer");

        ctx("alkemis.testnet", 1, 1_000);
        c.ft_transfer(test_account("bob.testnet"), U128(big), None);
        drain_logs(&mut all);
        ctx("alkemis.testnet", 0, 1_000);
        c.ft_burn(s2, U128(big));
        drain_logs(&mut all);

        let expected_transfer = format!(
            r#"EVENT_JSON:{{"standard":"nep141","version":"1.0.0","event":"ft_transfer","data":[{{"old_owner_id":"alkemis.testnet","new_owner_id":"bob.testnet","amount":"{}","memo":null}}]}}"#,
            big
        );
        assert!(
            all.iter().any(|l| l == &expected_transfer),
            "exact ft_transfer string-amount line missing:\n{expected_transfer}\nlogs: {:?}",
            all
        );
        let expected_burn = format!(
            r#"EVENT_JSON:{{"standard":"nep141","version":"1.0.0","event":"ft_burn","data":[{{"owner_id":"alkemis.testnet","amount":"{}","memo":null}}]}}"#,
            big
        );
        assert!(
            all.iter().any(|l| l == &expected_burn),
            "exact ft_burn string-amount line missing:\n{expected_burn}\nlogs: {:?}",
            all
        );
        for line in &all {
            if line.contains(r#""event":"ft_transfer""#) || line.contains(r#""event":"ft_burn""#) {
                let at = line.find(r#""amount":"#).expect("amount field present");
                assert_eq!(&line[at..at + 10], r#""amount":""#, "amount must be a JSON string");
            }
        }
        assert_eq!(sum_in(&all, "ft_transfer"), big, "2^53+1 survives string round-trip");
        assert_eq!(sum_in(&all, "ft_burn"), big, "2^53+1 survives string round-trip");
        assert_eq!(c.ft_balance_of(test_account("bob.testnet")).0, big);
    }
}
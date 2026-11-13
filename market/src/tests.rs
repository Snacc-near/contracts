//! EN-126 — unit tests for `snacc-market`: listing via `nft_on_transfer`, buy / delist /
//! reprice, owner pause, views, and the async edge cases (double buy, buy after delist,
//! failed transfer refund + owner re-check).
use super::*;
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{testing_env, PromiseResult};

const MARKET: &str = "market.testnet";
const NFT: &str = "snacc-v001.testnet";
const OWNER: &str = "owner.testnet";
const SELLER: &str = "seller.testnet";
const BUYER: &str = "buyer.testnet";
const OTHER: &str = "other.testnet";
const ONE_NEAR: u128 = 10u128.pow(24);
const NOW_NS: u64 = 1_763_000_000_000_000_000;

fn acct(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn ctx_full(predecessor: &str, deposit_yocto: u128, prepaid_tgas: u64, results: Vec<PromiseResult>) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acct(MARKET))
        .signer_account_id(acct(predecessor))
        .predecessor_account_id(acct(predecessor))
        .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
        .prepaid_gas(Gas::from_tgas(prepaid_tgas))
        .account_balance(NearToken::from_near(1_000_000))
        .block_timestamp(NOW_NS);
    testing_env!(
        b.build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        results
    );
}

fn ctx(predecessor: &str, deposit_yocto: u128) {
    ctx_full(predecessor, deposit_yocto, 300, vec![]);
}

/// A `#[private]` callback context (the market calling itself) with one promise result.
fn callback(result: PromiseResult) {
    ctx_full(MARKET, 0, 300, vec![result]);
}

fn setup() -> SnaccMarket {
    ctx(OWNER, 0);
    SnaccMarket::new(acct(NFT), acct(OWNER))
}

fn value(v: PromiseOrValue<bool>) -> bool {
    match v {
        PromiseOrValue::Value(b) => b,
        PromiseOrValue::Promise(_) => panic!("expected a value"),
    }
}

/// The NFT contract delivers `token_id` from `seller` with a raw `msg`; returns the
/// `nft_on_transfer` answer (`true` = token goes back).
fn on_transfer(m: &mut SnaccMarket, seller: &str, token_id: &str, msg: &str) -> bool {
    ctx(NFT, 0);
    value(m.nft_on_transfer(acct(seller), acct(seller), token_id.to_string(), msg.to_string()))
}

fn list(m: &mut SnaccMarket, seller: &str, token_id: &str, price: u128) -> bool {
    on_transfer(m, seller, token_id, &format!(r#"{{"price":"{price}"}}"#))
}

/// `data[0]` of every `snacc_market` event called `name` in the current context.
fn events(name: &str) -> Vec<serde_json::Value> {
    get_logs()
        .iter()
        .filter_map(|l| l.strip_prefix("EVENT_JSON:"))
        .filter_map(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .filter(|e| e["standard"] == EVENT_STANDARD && e["version"] == EVENT_VERSION && e["event"] == name)
        .map(|e| e["data"][0].clone())
        .collect()
}

/// Every NEAR transfer created in the current context: `(receiver, yocto)`.
fn transfers() -> Vec<(AccountId, u128)> {
    let mut out = vec![];
    for r in get_created_receipts() {
        for a in &r.actions {
            if let MockAction::Transfer { deposit, .. } = a {
                out.push((r.receiver_id.clone(), deposit.as_yoctonear()));
            }
        }
    }
    out
}

/// Every function call created in the current context: `(receiver, method, args, deposit, gas)`.
fn calls() -> Vec<(AccountId, String, serde_json::Value, u128, Gas)> {
    let mut out = vec![];
    for r in get_created_receipts() {
        for a in &r.actions {
            if let MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                prepaid_gas,
                ..
            } = a
            {
                out.push((
                    r.receiver_id.clone(),
                    String::from_utf8(method_name.clone()).unwrap(),
                    serde_json::from_slice(args).unwrap_or(serde_json::Value::Null),
                    attached_deposit.as_yoctonear(),
                    *prepaid_gas,
                ));
            }
        }
    }
    out
}

fn listing(token_id: &str, seller: &str, price: u128) -> Listing {
    Listing {
        token_id: token_id.to_string(),
        seller_id: acct(seller),
        price: U128(price),
        listed_at: U64(NOW_NS),
    }
}

fn owner_view(owner: &str) -> PromiseResult {
    // Shape of the Snacc `nft_token` view (extra fields are ignored by the market).
    PromiseResult::Successful(
        serde_json::to_vec(&json!({
            "token_id": "1",
            "owner_id": owner,
            "metadata": {"title": "Strain #1", "media": "data:image/svg+xml;base64,AAAA"}
        }))
        .unwrap(),
    )
}

// ----------------------------------------------------------------------
// init + listing
// ----------------------------------------------------------------------

#[test]
fn init_sets_config() {
    let m = setup();
    let cfg = m.get_config();
    assert_eq!(cfg.nft_contract_id, acct(NFT));
    assert_eq!(cfg.owner_id, acct(OWNER));
    assert!(!cfg.listings_paused);
    assert_eq!(cfg.fee_bps, 0);
    assert_eq!(cfg.max_listings_per_seller, 20);
    assert_eq!(cfg.max_listings_total, 1_000);
    assert_eq!(cfg.max_price, U128(MAX_PRICE_YOCTO));
    assert_eq!(m.get_listing_count(), 0);
}

#[test]
fn list_records_listing_and_emits_event() {
    let mut m = setup();
    // Measure inside one context (each `testing_env!` resets the usage counter);
    // `store` collections write on flush (at the end of a real call), so flush to measure.
    ctx(NFT, 0);
    let msg = format!(r#"{{"price":"{}"}}"#, 5 * ONE_NEAR);
    let before = env::storage_usage();
    let kept = !value(m.nft_on_transfer(acct(SELLER), acct(SELLER), "1".to_string(), msg.clone()));
    assert!(kept, "valid listing keeps the token");
    m.listings.flush();
    m.by_seller.flush();
    let first = env::storage_usage() - before;
    assert!(!value(m.nft_on_transfer(acct(SELLER), acct(SELLER), "2".to_string(), msg)));
    m.listings.flush();
    m.by_seller.flush();
    let second = env::storage_usage() - before - first;
    // The storage the market pays per listing (documented in market/README.md).
    println!("storage bytes per listing: first of a seller {first}, next {second}");
    assert!(first > 0 && first < 600, "first listing used {first} bytes");
    assert!(second > 0 && second < first, "next listing used {second} bytes");
    let _ = m.remove_listing("2");
    assert_eq!(m.get_listing("1".to_string()), Some(listing("1", SELLER, 5 * ONE_NEAR)));
    assert_eq!(m.get_listing_count(), 1);
    assert_eq!(m.get_listings_by_seller(acct(SELLER), None, None).len(), 1);
    let ev = events("listed");
    assert_eq!(ev.len(), 2);
    assert_eq!(ev[0]["token_id"], "1");
    assert_eq!(ev[0]["seller_id"], SELLER);
    assert_eq!(ev[0]["price"], (5 * ONE_NEAR).to_string());
    assert_eq!(ev[0]["listed_at"], NOW_NS.to_string());
}

#[test]
fn seller_is_previous_owner_not_sender() {
    let mut m = setup();
    ctx(NFT, 0);
    let kept = !value(m.nft_on_transfer(
        acct(OTHER),
        acct(SELLER),
        "9".to_string(),
        r#"{"price":"1"}"#.to_string(),
    ));
    assert!(kept);
    assert_eq!(m.get_listing("9".to_string()).unwrap().seller_id, acct(SELLER));
}

#[test]
#[should_panic(expected = "ERR_NOT_NFT_CONTRACT")]
fn on_transfer_from_other_contract_panics() {
    let mut m = setup();
    ctx(OTHER, 0);
    let _ = m.nft_on_transfer(acct(SELLER), acct(SELLER), "1".to_string(), r#"{"price":"1"}"#.to_string());
}

#[test]
fn invalid_msg_sends_token_back() {
    let mut m = setup();
    let too_high = format!(r#"{{"price":"{}"}}"#, MAX_PRICE_YOCTO + 1);
    for msg in [
        "",
        "not json",
        "{}",
        r#"{"price":1000}"#,
        r#"{"price":"0"}"#,
        r#"{"price":"-5"}"#,
        r#"{"price":"abc"}"#,
        r#"{"price":"1","extra":true}"#,
        too_high.as_str(),
    ] {
        assert!(on_transfer(&mut m, SELLER, "1", msg), "msg {msg:?} must be rejected");
        assert!(events("listed").is_empty());
    }
    assert_eq!(m.get_listing_count(), 0);
}

#[test]
fn price_cap_is_inclusive() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", MAX_PRICE_YOCTO));
    assert!(!list(&mut m, SELLER, "2", 1));
    assert_eq!(m.get_listing_count(), 2);
}

#[test]
fn same_token_twice_is_rejected() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    assert!(list(&mut m, OTHER, "1", ONE_NEAR));
    assert_eq!(m.get_listing("1".to_string()).unwrap().seller_id, acct(SELLER));
}

#[test]
fn seller_cap_of_20() {
    let mut m = setup();
    for i in 0..20 {
        assert!(!list(&mut m, SELLER, &i.to_string(), ONE_NEAR));
    }
    assert!(list(&mut m, SELLER, "20", ONE_NEAR), "21st listing goes back");
    assert!(!list(&mut m, OTHER, "21", ONE_NEAR), "other sellers are not affected");
    // Freeing a slot (delist) lets the seller list again.
    ctx(SELLER, 1);
    let _ = m.delist("0".to_string());
    assert!(!list(&mut m, SELLER, "20", ONE_NEAR));
    assert_eq!(m.get_listings_by_seller(acct(SELLER), None, None).len(), 20);
}

#[test]
fn market_total_cap() {
    let mut m = setup();
    let mut token = 0u32;
    for s in 0..(MAX_LISTINGS_TOTAL as usize / MAX_LISTINGS_PER_SELLER) {
        let seller = format!("seller{s}.testnet");
        for _ in 0..MAX_LISTINGS_PER_SELLER {
            assert!(!list(&mut m, &seller, &token.to_string(), ONE_NEAR));
            token += 1;
        }
    }
    assert_eq!(m.get_listing_count(), u64::from(MAX_LISTINGS_TOTAL));
    assert!(list(&mut m, "late.testnet", "999999", ONE_NEAR), "market full → token goes back");
}

// ----------------------------------------------------------------------
// pause (owner)
// ----------------------------------------------------------------------

#[test]
fn pause_blocks_new_listings_only() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    assert!(!list(&mut m, SELLER, "2", ONE_NEAR));
    assert!(!list(&mut m, SELLER, "3", ONE_NEAR));
    ctx(OWNER, 1);
    m.set_listings_paused(true);
    assert!(m.get_config().listings_paused);
    assert!(list(&mut m, SELLER, "4", ONE_NEAR), "paused → token goes back");
    // Existing listings still work.
    ctx(SELLER, 1);
    m.update_price("1".to_string(), U128(2 * ONE_NEAR));
    ctx(BUYER, 2 * ONE_NEAR);
    let _ = m.buy("1".to_string());
    ctx(SELLER, 1);
    let _ = m.delist("2".to_string());
    assert_eq!(m.get_listing_count(), 1);
    ctx(OWNER, 1);
    m.set_listings_paused(false);
    assert!(!list(&mut m, SELLER, "4", ONE_NEAR));
}

#[test]
#[should_panic(expected = "ERR_NOT_OWNER")]
fn pause_by_non_owner_panics() {
    let mut m = setup();
    ctx(SELLER, 1);
    m.set_listings_paused(true);
}

#[test]
#[should_panic(expected = "ERR_YOCTO_REQUIRED")]
fn pause_without_yocto_panics() {
    let mut m = setup();
    ctx(OWNER, 0);
    m.set_listings_paused(true);
}

// ----------------------------------------------------------------------
// buy
// ----------------------------------------------------------------------

#[test]
fn buy_success_pays_seller_and_refunds_excess() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", 5 * ONE_NEAR));
    ctx(BUYER, 7 * ONE_NEAR);
    let _ = m.buy("1".to_string());
    assert_eq!(m.get_listing("1".to_string()), None, "removed before the cross-contract call");
    assert_eq!(m.get_listing_count(), 0);
    assert!(m.get_listings_by_seller(acct(SELLER), None, None).is_empty());
    let c = calls();
    let (rcv, method, args, dep, gas) = &c[0];
    assert_eq!(rcv, &acct(NFT));
    assert_eq!(method, "nft_transfer");
    assert_eq!(args["receiver_id"], BUYER);
    assert_eq!(args["token_id"], "1");
    assert!(args["approval_id"].is_null());
    assert_eq!(*dep, 1, "exactly 1 yoctoNEAR to nft_transfer");
    assert!(*gas >= GAS_FOR_NFT_TRANSFER);
    let (rcv, method, args, _, gas) = &c[1];
    assert_eq!(rcv, &acct(MARKET));
    assert_eq!(method, "on_buy_settled");
    assert_eq!(args["buyer_id"], BUYER);
    assert_eq!(args["deposit"], (7 * ONE_NEAR).to_string());
    assert!(*gas >= GAS_FOR_ON_TRANSFER_SETTLED);
    assert!(transfers().is_empty(), "nobody is paid before the transfer settles");

    callback(PromiseResult::Successful(vec![]));
    assert!(m.on_buy_settled(listing("1", SELLER, 5 * ONE_NEAR), acct(BUYER), U128(7 * ONE_NEAR)));
    let t = transfers();
    assert_eq!(t, vec![(acct(SELLER), 5 * ONE_NEAR), (acct(BUYER), 2 * ONE_NEAR)]);
    let ev = events("sold");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["token_id"], "1");
    assert_eq!(ev[0]["seller_id"], SELLER);
    assert_eq!(ev[0]["buyer_id"], BUYER);
    assert_eq!(ev[0]["price"], (5 * ONE_NEAR).to_string());
}

#[test]
fn buy_exact_price_has_no_refund() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", 5 * ONE_NEAR));
    ctx(BUYER, 5 * ONE_NEAR);
    let _ = m.buy("1".to_string());
    callback(PromiseResult::Successful(vec![]));
    assert!(m.on_buy_settled(listing("1", SELLER, 5 * ONE_NEAR), acct(BUYER), U128(5 * ONE_NEAR)));
    assert_eq!(transfers(), vec![(acct(SELLER), 5 * ONE_NEAR)]);
}

#[test]
#[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
fn buy_with_too_little_deposit_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", 5 * ONE_NEAR));
    ctx(BUYER, 5 * ONE_NEAR - 1);
    let _ = m.buy("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_LISTING_NOT_FOUND")]
fn buy_unknown_listing_panics() {
    let mut m = setup();
    ctx(BUYER, ONE_NEAR);
    let _ = m.buy("404".to_string());
}

#[test]
#[should_panic(expected = "ERR_SELLER_CANNOT_BUY")]
fn seller_cannot_buy_own_listing() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, ONE_NEAR);
    let _ = m.buy("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_MORE_GAS_REQUIRED")]
fn buy_with_low_gas_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx_full(BUYER, ONE_NEAR, GAS_REQUIRED_FOR_TRANSFER_FLOW.as_tgas() - 1, vec![]);
    let _ = m.buy("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_LISTING_NOT_FOUND")]
fn double_buy_second_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(BUYER, ONE_NEAR);
    let _ = m.buy("1".to_string());
    // A second buyer in the same window (the first transfer has not settled yet).
    ctx(OTHER, ONE_NEAR);
    let _ = m.buy("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_LISTING_NOT_FOUND")]
fn buy_after_delist_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    let _ = m.delist("1".to_string());
    ctx(BUYER, ONE_NEAR);
    let _ = m.buy("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_LISTING_NOT_FOUND")]
fn delist_during_pending_buy_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(BUYER, ONE_NEAR);
    let _ = m.buy("1".to_string());
    ctx(SELLER, 1);
    let _ = m.delist("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_LISTING_NOT_FOUND")]
fn update_price_during_pending_buy_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(BUYER, ONE_NEAR);
    let _ = m.buy("1".to_string());
    ctx(SELLER, 1);
    m.update_price("1".to_string(), U128(1));
}

#[test]
fn failed_transfer_refunds_buyer_and_restores_listing_when_still_owned() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", 5 * ONE_NEAR));
    ctx(BUYER, 6 * ONE_NEAR);
    let _ = m.buy("1".to_string());
    callback(PromiseResult::Failed);
    let l = listing("1", SELLER, 5 * ONE_NEAR);
    assert!(!m.on_buy_settled(l.clone(), acct(BUYER), U128(6 * ONE_NEAR)));
    assert_eq!(transfers(), vec![(acct(BUYER), 6 * ONE_NEAR)], "full refund, seller unpaid");
    assert!(events("sold").is_empty());
    let c = calls();
    assert_eq!(c[0].0, acct(NFT));
    assert_eq!(c[0].1, "nft_token");
    assert_eq!(c[0].2["token_id"], "1");
    assert_eq!(c[1].1, "on_owner_checked");
    assert_eq!(m.get_listing("1".to_string()), None, "not restored until the re-check");

    callback(owner_view(MARKET));
    assert!(m.on_owner_checked(l.clone()));
    assert_eq!(m.get_listing("1".to_string()), Some(l));
    assert_eq!(m.get_listings_by_seller(acct(SELLER), None, None).len(), 1);
    assert!(events("delisted").is_empty());
}

#[test]
fn failed_transfer_keeps_listing_removed_when_token_left_market() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(BUYER, ONE_NEAR);
    let _ = m.buy("1".to_string());
    callback(PromiseResult::Failed);
    let l = listing("1", SELLER, ONE_NEAR);
    assert!(!m.on_buy_settled(l.clone(), acct(BUYER), U128(ONE_NEAR)));
    // e.g. the strain starved while listed and was adopted by someone else.
    callback(owner_view(OTHER));
    assert!(!m.on_owner_checked(l.clone()));
    assert_eq!(m.get_listing("1".to_string()), None);
    assert_eq!(m.get_listing_count(), 0);
    let ev = events("delisted");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["reason"], "token_left_market");
    // A `null` token view (burned token) is "not in the market" too.
    callback(PromiseResult::Successful(b"null".to_vec()));
    assert!(!m.on_owner_checked(l));
    assert_eq!(m.get_listing_count(), 0);
}

#[test]
fn owner_check_view_failure_restores_listing() {
    let mut m = setup();
    let l = listing("1", SELLER, ONE_NEAR);
    callback(PromiseResult::Failed);
    assert!(m.on_owner_checked(l.clone()));
    assert_eq!(m.get_listing("1".to_string()), Some(l.clone()));
    // Restoring twice never duplicates the seller index.
    callback(owner_view(MARKET));
    assert!(!m.on_owner_checked(l));
    assert_eq!(m.get_listings_by_seller(acct(SELLER), None, None).len(), 1);
}

#[test]
fn buyer_can_relist_after_sale() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(BUYER, ONE_NEAR);
    let _ = m.buy("1".to_string());
    callback(PromiseResult::Successful(vec![]));
    assert!(m.on_buy_settled(listing("1", SELLER, ONE_NEAR), acct(BUYER), U128(ONE_NEAR)));
    assert!(!list(&mut m, BUYER, "1", 3 * ONE_NEAR));
    assert_eq!(m.get_listing("1".to_string()).unwrap().seller_id, acct(BUYER));
}

// ----------------------------------------------------------------------
// delist
// ----------------------------------------------------------------------

#[test]
fn delist_returns_token_and_emits_after_success() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    let _ = m.delist("1".to_string());
    assert_eq!(m.get_listing("1".to_string()), None);
    assert!(m.get_listings_by_seller(acct(SELLER), None, None).is_empty());
    let c = calls();
    assert_eq!(c[0].0, acct(NFT));
    assert_eq!(c[0].1, "nft_transfer");
    assert_eq!(c[0].2["receiver_id"], SELLER);
    assert_eq!(c[0].3, 1);
    assert_eq!(c[1].1, "on_delist_settled");
    assert!(events("delisted").is_empty(), "emitted only once the transfer succeeds");

    callback(PromiseResult::Successful(vec![]));
    assert!(m.on_delist_settled(listing("1", SELLER, ONE_NEAR)));
    let ev = events("delisted");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["reason"], "seller");
    assert!(transfers().is_empty());
}

#[test]
fn failed_delist_rechecks_and_restores() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    let _ = m.delist("1".to_string());
    callback(PromiseResult::Failed);
    let l = listing("1", SELLER, ONE_NEAR);
    assert!(!m.on_delist_settled(l.clone()));
    assert_eq!(calls()[0].1, "nft_token");
    callback(owner_view(MARKET));
    assert!(m.on_owner_checked(l.clone()));
    assert_eq!(m.get_listing("1".to_string()), Some(l));
}

#[test]
#[should_panic(expected = "ERR_NOT_SELLER")]
fn delist_by_non_seller_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(OTHER, 1);
    let _ = m.delist("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_YOCTO_REQUIRED")]
fn delist_without_yocto_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 0);
    let _ = m.delist("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_LISTING_NOT_FOUND")]
fn double_delist_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    let _ = m.delist("1".to_string());
    ctx(SELLER, 1);
    let _ = m.delist("1".to_string());
}

// ----------------------------------------------------------------------
// update_price
// ----------------------------------------------------------------------

#[test]
fn update_price_by_seller() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    m.update_price("1".to_string(), U128(3 * ONE_NEAR));
    assert_eq!(m.get_listing("1".to_string()).unwrap().price, U128(3 * ONE_NEAR));
    let ev = events("price_updated");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["old_price"], ONE_NEAR.to_string());
    assert_eq!(ev[0]["new_price"], (3 * ONE_NEAR).to_string());
}

#[test]
#[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
fn buy_after_price_increase_needs_new_price() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    m.update_price("1".to_string(), U128(3 * ONE_NEAR));
    ctx(BUYER, 3 * ONE_NEAR - 1);
    let _ = m.buy("1".to_string());
}

#[test]
#[should_panic(expected = "ERR_NOT_SELLER")]
fn update_price_by_non_seller_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(OTHER, 1);
    m.update_price("1".to_string(), U128(1));
}

#[test]
#[should_panic(expected = "ERR_BAD_PRICE")]
fn update_price_to_zero_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    m.update_price("1".to_string(), U128(0));
}

#[test]
#[should_panic(expected = "ERR_BAD_PRICE")]
fn update_price_above_cap_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 1);
    m.update_price("1".to_string(), U128(MAX_PRICE_YOCTO + 1));
}

#[test]
#[should_panic(expected = "ERR_YOCTO_REQUIRED")]
fn update_price_without_yocto_panics() {
    let mut m = setup();
    assert!(!list(&mut m, SELLER, "1", ONE_NEAR));
    ctx(SELLER, 0);
    m.update_price("1".to_string(), U128(2));
}

// ----------------------------------------------------------------------
// views
// ----------------------------------------------------------------------

#[test]
fn views_paginate() {
    let mut m = setup();
    for t in ["1", "2", "3"] {
        assert!(!list(&mut m, SELLER, t, ONE_NEAR));
    }
    for t in ["4", "5"] {
        assert!(!list(&mut m, OTHER, t, 2 * ONE_NEAR));
    }
    assert_eq!(m.get_listing_count(), 5);
    assert_eq!(m.get_listings(None, None).len(), 5);
    let page2: Vec<String> = m
        .get_listings(Some(U128(1)), Some(2))
        .into_iter()
        .map(|l| l.token_id)
        .collect();
    assert_eq!(page2, vec!["2".to_string(), "3".to_string()]);
    assert!(m.get_listings(Some(U128(10)), None).is_empty());
    assert!(m.get_listings(Some(U128(u128::MAX)), None).is_empty());
    let by: Vec<String> = m
        .get_listings_by_seller(acct(SELLER), Some(U128(1)), Some(1))
        .into_iter()
        .map(|l| l.token_id)
        .collect();
    assert_eq!(by, vec!["2".to_string()]);
    assert_eq!(m.get_listings_by_seller(acct(OTHER), None, None).len(), 2);
    assert!(m.get_listings_by_seller(acct(BUYER), None, None).is_empty());
    assert_eq!(m.get_listing("404".to_string()), None);
    assert_eq!(page(None, Some(10_000)), (0, MAX_PAGE_LIMIT as usize));
    assert_eq!(page(None, None), (0, DEFAULT_PAGE_LIMIT as usize));
}

//! `snacc-market` — a small testnet marketplace for Snacc strains (`EN-126`).
//!
//! * **List:** the owner of a strain calls the Snacc contract's NEP-171
//!   `nft_transfer_call(receiver_id = <market>, token_id, msg = {"price":"<yoctoNEAR>"})`.
//!   The Snacc contract calls `nft_on_transfer` here; a valid listing keeps the token in
//!   escrow (returns `false`), anything invalid sends it back (returns `true`).
//! * **Buy:** `buy(token_id)` with at least `price` attached. The listing is removed
//!   first, the token is sent to the buyer, and only after that transfer succeeds is the
//!   seller paid `price` (the excess is refunded). If the transfer fails, the buyer gets
//!   the full deposit back and the listing is restored only while the market still owns
//!   the token.
//! * **Delist / reprice:** only the seller, with exactly 1 yoctoNEAR attached.
//!
//! 0% fee. There is no admin withdrawal of listed tokens; the owner can only pause NEW
//! listings. Testnet only: testnet NEAR and testnet `$SNACC` have no money value.

use near_sdk::json_types::{U128, U64};
use near_sdk::serde::Deserialize;
use near_sdk::serde_json::{self, json};
use near_sdk::store::{IterableMap, LookupMap};
use near_sdk::{
    env, ext_contract, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise,
    PromiseError, PromiseOrValue,
};

/// The NEP-297 `standard` of every market event.
pub const EVENT_STANDARD: &str = "snacc_market";
/// The NEP-297 `version` of every market event.
pub const EVENT_VERSION: &str = "1.0.0";

/// At most this many open listings per seller (bounds the storage the market pays for).
pub const MAX_LISTINGS_PER_SELLER: usize = 20;
/// At most this many open listings in the whole market (bounds the storage the market pays for).
pub const MAX_LISTINGS_TOTAL: u32 = 1_000;
/// Price cap: 10,000 (testnet) NEAR in yoctoNEAR. A price must be `> 0` and `<=` this.
pub const MAX_PRICE_YOCTO: u128 = 10_000 * 10u128.pow(24);
/// Default and maximum page size of the list views.
pub const DEFAULT_PAGE_LIMIT: u64 = 50;
pub const MAX_PAGE_LIMIT: u64 = 100;

/// Gas for the Snacc contract's `nft_transfer` (owner check + colony index move + event).
pub const GAS_FOR_NFT_TRANSFER: Gas = Gas::from_tgas(30);
/// Gas for the Snacc contract's `nft_token` view (renders the live SVG metadata).
pub const GAS_FOR_NFT_TOKEN: Gas = Gas::from_tgas(30);
/// Gas for `on_owner_checked` (restore or drop the listing).
pub const GAS_FOR_ON_OWNER_CHECKED: Gas = Gas::from_tgas(15);
/// Gas for `on_buy_settled` / `on_delist_settled`: own work + a possible owner re-check.
pub const GAS_FOR_ON_TRANSFER_SETTLED: Gas =
    Gas::from_tgas(15 + GAS_FOR_NFT_TOKEN.as_tgas() + GAS_FOR_ON_OWNER_CHECKED.as_tgas());
/// Minimum prepaid gas for `buy` / `delist` (own work + transfer + settle callback).
/// Wallet calls should attach 150 TGas or more.
pub const GAS_REQUIRED_FOR_TRANSFER_FLOW: Gas = Gas::from_tgas(
    20 + GAS_FOR_NFT_TRANSFER.as_tgas() + GAS_FOR_ON_TRANSFER_SETTLED.as_tgas(),
);
/// Upper bound read from the `nft_token` view result (the SVG metadata is a few KB).
const MAX_NFT_TOKEN_VIEW_BYTES: usize = 32 * 1024;

pub const ERR_NOT_NFT_CONTRACT: &str = "ERR_NOT_NFT_CONTRACT";
pub const ERR_NOT_OWNER: &str = "ERR_NOT_OWNER";
pub const ERR_NOT_SELLER: &str = "ERR_NOT_SELLER";
pub const ERR_YOCTO_REQUIRED: &str = "ERR_YOCTO_REQUIRED";
pub const ERR_LISTING_NOT_FOUND: &str = "ERR_LISTING_NOT_FOUND";
pub const ERR_SELLER_CANNOT_BUY: &str = "ERR_SELLER_CANNOT_BUY";
pub const ERR_INSUFFICIENT_DEPOSIT: &str = "ERR_INSUFFICIENT_DEPOSIT";
pub const ERR_BAD_PRICE: &str = "ERR_BAD_PRICE";
pub const ERR_MORE_GAS_REQUIRED: &str = "ERR_MORE_GAS_REQUIRED";
/// Reasons logged (not panicked) when `nft_on_transfer` sends a token back.
pub const ERR_LISTINGS_PAUSED: &str = "ERR_LISTINGS_PAUSED";
pub const ERR_BAD_MSG: &str = "ERR_BAD_MSG";
pub const ERR_ALREADY_LISTED: &str = "ERR_ALREADY_LISTED";
pub const ERR_SELLER_LISTING_CAP: &str = "ERR_SELLER_LISTING_CAP";
pub const ERR_MARKET_FULL: &str = "ERR_MARKET_FULL";

/// One open listing. The token is held by the market until `buy` or `delist`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    pub token_id: String,
    /// The `previous_owner_id` of the listing `nft_transfer_call` — the account paid on sale.
    pub seller_id: AccountId,
    /// Price in yoctoNEAR (decimal string in JSON).
    pub price: U128,
    /// Block timestamp of the listing, nanoseconds (decimal string in JSON).
    pub listed_at: U64,
}

/// `get_config` view.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
pub struct MarketConfig {
    pub nft_contract_id: AccountId,
    pub owner_id: AccountId,
    pub listings_paused: bool,
    pub fee_bps: u16,
    pub max_listings_per_seller: u32,
    pub max_listings_total: u32,
    pub max_price: U128,
}

/// The `nft_on_transfer` `msg`: exactly `{"price":"<yoctoNEAR U128 string>"}`.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct ListingMsg {
    price: U128,
}

/// The only field the market reads from the NFT contract's `nft_token` view.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct TokenOwnerView {
    owner_id: AccountId,
}

/// The NEP-171 calls the market makes on the NFT contract.
#[ext_contract(ext_nft)]
#[allow(dead_code)] // only the generated `ext_nft` module is used
trait NftContract {
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
    );
    fn nft_token(&self, token_id: String);
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct SnaccMarket {
    nft_contract_id: AccountId,
    owner_id: AccountId,
    listings_paused: bool,
    /// token_id → listing (iterable for `get_listings`).
    listings: IterableMap<String, Listing>,
    /// seller → token ids of their open listings (at most `MAX_LISTINGS_PER_SELLER`,
    /// except a listing restored after a failed sale, see `on_owner_checked`).
    by_seller: LookupMap<AccountId, Vec<String>>,
}

fn emit_event(event: &str, data: serde_json::Value) {
    env::log_str(&format!(
        r#"EVENT_JSON:{{"standard":"{}","version":"{}","event":"{}","data":[{}]}}"#,
        EVENT_STANDARD, EVENT_VERSION, event, data
    ));
}

fn assert_one_yocto() {
    require!(
        env::attached_deposit() == NearToken::from_yoctonear(1),
        ERR_YOCTO_REQUIRED
    );
}

fn is_valid_price(price: u128) -> bool {
    price > 0 && price <= MAX_PRICE_YOCTO
}

/// `true` unless promise result 0 failed (an empty or any other successful result counts).
fn first_promise_succeeded() -> bool {
    !matches!(env::promise_result_checked(0, 0), Err(PromiseError::Failed))
}

#[near]
impl SnaccMarket {
    /// `nft_contract_id` = the Snacc NFT contract (the only accepted `nft_on_transfer`
    /// caller); `owner_id` = the only account that can pause new listings.
    #[init]
    pub fn new(nft_contract_id: AccountId, owner_id: AccountId) -> Self {
        Self {
            nft_contract_id,
            owner_id,
            listings_paused: false,
            listings: IterableMap::new(b"l".to_vec()),
            by_seller: LookupMap::new(b"s".to_vec()),
        }
    }

    // ------------------------------------------------------------------
    // NEP-171 receiver — listing
    // ------------------------------------------------------------------

    /// NEP-171 `nft_on_transfer`. Only the NFT contract may call it. Returns `false`
    /// (keep the token) when the listing is recorded, `true` (send it back) otherwise.
    pub fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: String,
        msg: String,
    ) -> PromiseOrValue<bool> {
        require!(
            env::predecessor_account_id() == self.nft_contract_id,
            ERR_NOT_NFT_CONTRACT
        );
        let _ = sender_id;
        match self.try_list(previous_owner_id, token_id, &msg) {
            Ok(()) => PromiseOrValue::Value(false),
            Err(reason) => {
                env::log_str(&format!("snacc_market: listing rejected ({reason}), token returned"));
                PromiseOrValue::Value(true)
            }
        }
    }

    fn try_list(
        &mut self,
        seller_id: AccountId,
        token_id: String,
        msg: &str,
    ) -> Result<(), &'static str> {
        if self.listings_paused {
            return Err(ERR_LISTINGS_PAUSED);
        }
        let parsed: ListingMsg = serde_json::from_str(msg).map_err(|_| ERR_BAD_MSG)?;
        if !is_valid_price(parsed.price.0) {
            return Err(ERR_BAD_PRICE);
        }
        if self.listings.contains_key(&token_id) {
            return Err(ERR_ALREADY_LISTED);
        }
        if self.listings.len() >= MAX_LISTINGS_TOTAL {
            return Err(ERR_MARKET_FULL);
        }
        let open = self.by_seller.get(&seller_id).map_or(0, Vec::len);
        if open >= MAX_LISTINGS_PER_SELLER {
            return Err(ERR_SELLER_LISTING_CAP);
        }
        let listing = Listing {
            token_id,
            seller_id,
            price: parsed.price,
            listed_at: U64(env::block_timestamp()),
        };
        emit_event(
            "listed",
            json!({
                "token_id": listing.token_id,
                "seller_id": listing.seller_id,
                "price": listing.price,
                "listed_at": listing.listed_at,
            }),
        );
        self.insert_listing(listing);
        Ok(())
    }

    fn insert_listing(&mut self, listing: Listing) {
        match self.by_seller.get_mut(&listing.seller_id) {
            Some(ids) => ids.push(listing.token_id.clone()),
            None => {
                let _ = self
                    .by_seller
                    .insert(listing.seller_id.clone(), vec![listing.token_id.clone()]);
            }
        }
        let _ = self.listings.insert(listing.token_id.clone(), listing);
    }

    fn remove_listing(&mut self, token_id: &str) -> Option<Listing> {
        let listing = self.listings.remove(token_id)?;
        let now_empty = match self.by_seller.get_mut(&listing.seller_id) {
            Some(ids) => {
                ids.retain(|id| id != token_id);
                ids.is_empty()
            }
            None => false,
        };
        if now_empty {
            let _ = self.by_seller.remove(&listing.seller_id);
        }
        Some(listing)
    }

    fn require_transfer_gas() {
        require!(
            env::prepaid_gas() >= GAS_REQUIRED_FOR_TRANSFER_FLOW,
            ERR_MORE_GAS_REQUIRED
        );
    }

    // ------------------------------------------------------------------
    // Buy / delist / reprice
    // ------------------------------------------------------------------

    /// Buy a listed token with at least `price` yoctoNEAR attached. The listing is removed
    /// before any cross-contract call, so a second `buy`/`delist`/`update_price` in the
    /// same window fails with `ERR_LISTING_NOT_FOUND`. The seller cannot buy their own
    /// listing. Needs `GAS_REQUIRED_FOR_TRANSFER_FLOW` (attach 150 TGas or more).
    #[payable]
    pub fn buy(&mut self, token_id: String) -> Promise {
        Self::require_transfer_gas();
        let buyer_id = env::predecessor_account_id();
        let deposit = env::attached_deposit().as_yoctonear();
        let listing = self
            .listings
            .get(&token_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str(ERR_LISTING_NOT_FOUND));
        require!(buyer_id != listing.seller_id, ERR_SELLER_CANNOT_BUY);
        require!(deposit >= listing.price.0, ERR_INSUFFICIENT_DEPOSIT);
        let _ = self.remove_listing(&token_id);
        ext_nft::ext(self.nft_contract_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_FOR_NFT_TRANSFER)
            .nft_transfer(
                buyer_id.clone(),
                token_id,
                None,
                Some("snacc_market sale".to_string()),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_ON_TRANSFER_SETTLED)
                    .on_buy_settled(listing, buyer_id, U128(deposit)),
            )
    }

    /// `buy` callback. Transfer succeeded → pay the seller `price`, refund the excess,
    /// `sold` event, `true`. Transfer failed → refund the full deposit to the buyer, pay
    /// the seller nothing, re-check who owns the token (`on_owner_checked`), `false`.
    #[private]
    pub fn on_buy_settled(&mut self, listing: Listing, buyer_id: AccountId, deposit: U128) -> bool {
        if first_promise_succeeded() {
            let _ = Promise::new(listing.seller_id.clone())
                .transfer(NearToken::from_yoctonear(listing.price.0));
            let excess = deposit.0.saturating_sub(listing.price.0);
            if excess > 0 {
                let _ = Promise::new(buyer_id.clone()).transfer(NearToken::from_yoctonear(excess));
            }
            emit_event(
                "sold",
                json!({
                    "token_id": listing.token_id,
                    "seller_id": listing.seller_id,
                    "buyer_id": buyer_id,
                    "price": listing.price,
                }),
            );
            return true;
        }
        let _ = Promise::new(buyer_id).transfer(NearToken::from_yoctonear(deposit.0));
        let _ = self.recheck_owner(listing);
        false
    }

    /// Take a listing back (seller only, exactly 1 yoctoNEAR). The listing is removed and
    /// the token is sent back to the seller; `delisted` is emitted once that succeeds.
    /// Needs `GAS_REQUIRED_FOR_TRANSFER_FLOW` (attach 150 TGas or more).
    #[payable]
    pub fn delist(&mut self, token_id: String) -> Promise {
        assert_one_yocto();
        Self::require_transfer_gas();
        let listing = self
            .listings
            .get(&token_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str(ERR_LISTING_NOT_FOUND));
        require!(
            env::predecessor_account_id() == listing.seller_id,
            ERR_NOT_SELLER
        );
        let _ = self.remove_listing(&token_id);
        ext_nft::ext(self.nft_contract_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_FOR_NFT_TRANSFER)
            .nft_transfer(
                listing.seller_id.clone(),
                token_id,
                None,
                Some("snacc_market delist".to_string()),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_ON_TRANSFER_SETTLED)
                    .on_delist_settled(listing),
            )
    }

    /// `delist` callback: success → `delisted` (`reason: "seller"`), `true`; failure →
    /// re-check who owns the token (`on_owner_checked`), `false`.
    #[private]
    pub fn on_delist_settled(&mut self, listing: Listing) -> bool {
        if first_promise_succeeded() {
            emit_event(
                "delisted",
                json!({
                    "token_id": listing.token_id,
                    "seller_id": listing.seller_id,
                    "reason": "seller",
                }),
            );
            return true;
        }
        let _ = self.recheck_owner(listing);
        false
    }

    /// Ask the NFT contract who owns the token after a failed transfer out.
    fn recheck_owner(&self, listing: Listing) -> Promise {
        ext_nft::ext(self.nft_contract_id.clone())
            .with_static_gas(GAS_FOR_NFT_TOKEN)
            .nft_token(listing.token_id.clone())
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_ON_OWNER_CHECKED)
                    .on_owner_checked(listing),
            )
    }

    /// Owner re-check after a failed `buy`/`delist` transfer. The market still owns the
    /// token → the listing is restored as it was (`true`; this may take the seller one
    /// over `MAX_LISTINGS_PER_SELLER`, since the slot was freed during the window). The
    /// token left the market (e.g. it was adopted while listed) → the listing stays
    /// removed and `delisted` (`reason: "token_left_market"`) is emitted (`false`).
    /// The view itself failed → treated as "still owned" (a stale listing can only make
    /// a later `buy` fail with a full refund; dropping it could strand the seller's token).
    #[private]
    pub fn on_owner_checked(&mut self, listing: Listing) -> bool {
        let still_in_market = match env::promise_result_checked(0, MAX_NFT_TOKEN_VIEW_BYTES) {
            Ok(bytes) => match serde_json::from_slice::<Option<TokenOwnerView>>(&bytes) {
                Ok(Some(t)) => t.owner_id == env::current_account_id(),
                Ok(None) => false,
                Err(_) => true,
            },
            Err(_) => true,
        };
        if !still_in_market {
            emit_event(
                "delisted",
                json!({
                    "token_id": listing.token_id,
                    "seller_id": listing.seller_id,
                    "reason": "token_left_market",
                }),
            );
            return false;
        }
        if self.listings.contains_key(&listing.token_id) {
            return false;
        }
        self.insert_listing(listing);
        true
    }

    /// Change the price of an open listing (seller only, exactly 1 yoctoNEAR — so a
    /// function-call access key alone cannot reprice). `price_updated` event.
    #[payable]
    pub fn update_price(&mut self, token_id: String, price: U128) {
        assert_one_yocto();
        require!(is_valid_price(price.0), ERR_BAD_PRICE);
        let caller = env::predecessor_account_id();
        let listing = self
            .listings
            .get_mut(&token_id)
            .unwrap_or_else(|| env::panic_str(ERR_LISTING_NOT_FOUND));
        require!(caller == listing.seller_id, ERR_NOT_SELLER);
        let old_price = listing.price;
        listing.price = price;
        emit_event(
            "price_updated",
            json!({
                "token_id": token_id,
                "seller_id": caller,
                "old_price": old_price,
                "new_price": price,
            }),
        );
    }

    // ------------------------------------------------------------------
    // Owner — pause new listings only
    // ------------------------------------------------------------------

    /// Pause or resume NEW listings (owner only, exactly 1 yoctoNEAR). `buy`, `delist`,
    /// and `update_price` keep working while paused, so no listed token is ever stuck.
    #[payable]
    pub fn set_listings_paused(&mut self, paused: bool) {
        assert_one_yocto();
        require!(env::predecessor_account_id() == self.owner_id, ERR_NOT_OWNER);
        self.listings_paused = paused;
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_listing(&self, token_id: String) -> Option<Listing> {
        self.listings.get(&token_id).cloned()
    }

    /// NEP-181-style pagination: `from_index` (default 0), `limit` (default 50, max 100).
    pub fn get_listings(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Listing> {
        let (start, take) = page(from_index, limit);
        self.listings
            .values()
            .skip(start)
            .take(take)
            .cloned()
            .collect()
    }

    pub fn get_listings_by_seller(
        &self,
        seller_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Listing> {
        let (start, take) = page(from_index, limit);
        self.by_seller
            .get(&seller_id)
            .map(|ids| {
                ids.iter()
                    .skip(start)
                    .take(take)
                    .filter_map(|id| self.listings.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_listing_count(&self) -> u64 {
        u64::from(self.listings.len())
    }

    pub fn get_config(&self) -> MarketConfig {
        MarketConfig {
            nft_contract_id: self.nft_contract_id.clone(),
            owner_id: self.owner_id.clone(),
            listings_paused: self.listings_paused,
            fee_bps: 0,
            max_listings_per_seller: MAX_LISTINGS_PER_SELLER as u32,
            max_listings_total: MAX_LISTINGS_TOTAL,
            max_price: U128(MAX_PRICE_YOCTO),
        }
    }
}

fn page(from_index: Option<U128>, limit: Option<u64>) -> (usize, usize) {
    let start = usize::try_from(from_index.map_or(0, |i| i.0)).unwrap_or(usize::MAX);
    let take = limit.unwrap_or(DEFAULT_PAGE_LIMIT).min(MAX_PAGE_LIMIT) as usize;
    (start, take)
}

#[cfg(test)]
mod tests;

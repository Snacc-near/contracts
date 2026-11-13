# market — `snacc-market` (EN-126)

A small **testnet-only** marketplace for Snacc strains (the NEP-171 NFTs of the Snacc contract).
A strain is listed by sending it to the market with the Snacc contract's NEP-171
`nft_transfer_call`; the market holds it in escrow until someone buys it with NEAR or the
seller takes it back.

> **Testnet only, no money value.** This contract is meant for `testnet`. Testnet NEAR and
> testnet `$SNACC` have no monetary value. There is no fee (0%), no promise of profit, and
> no admin withdrawal of listed tokens.

## Methods

| Method | Who | Deposit | Notes |
| --- | --- | --- | --- |
| `new(nft_contract_id, owner_id)` | deployer (init) | — | `nft_contract_id` = the Snacc contract; `owner_id` can only pause new listings |
| `nft_on_transfer(sender_id, previous_owner_id, token_id, msg)` | the Snacc contract only | — | NEP-171 receiver. Valid `msg` → listing recorded, returns `false` (token kept). Invalid → returns `true` (token goes back). Any other caller → `ERR_NOT_NFT_CONTRACT` |
| `buy(token_id)` | anyone except the seller | `>= price` yoctoNEAR | Listing removed first; token sent to the buyer; then the seller is paid `price` and the excess is refunded. Attach **150 TGas or more** (minimum 110 TGas) |
| `delist(token_id)` | seller | exactly 1 yoctoNEAR | Listing removed; token sent back to the seller. Attach **150 TGas or more** |
| `update_price(token_id, price)` | seller | exactly 1 yoctoNEAR | New price, same rules as listing |
| `set_listings_paused(paused)` | owner | exactly 1 yoctoNEAR | Pauses NEW listings only; `buy`, `delist`, `update_price` keep working |
| `get_listing(token_id)` | view | — | `Listing` or `null` |
| `get_listings(from_index, limit)` | view | — | NEP-181-style paging (`from_index` U128 string, default 0; `limit` default 50, max 100) |
| `get_listings_by_seller(seller_id, from_index, limit)` | view | — | Same paging, one seller |
| `get_listing_count()` | view | — | Number of open listings |
| `get_config()` | view | — | `nft_contract_id`, `owner_id`, `listings_paused`, `fee_bps` (0), caps, `max_price` |

`Listing` JSON: `{"token_id":"7","seller_id":"alice.testnet","price":"1000000000000000000000000","listed_at":"1763000000000000000"}`
(`price` in yoctoNEAR and `listed_at` in block-timestamp nanoseconds, both decimal strings).

The callbacks `on_buy_settled`, `on_delist_settled`, and `on_owner_checked` are `#[private]`.

## Listing message

Call the **Snacc contract** (1 yoctoNEAR attached, e.g. 100 TGas):

```bash
near contract call-function as-transaction snacc-v001.testnet nft_transfer_call \
  json-args '{"receiver_id":"<market account>","token_id":"7","msg":"{\"price\":\"1000000000000000000000000\"}"}' \
  prepaid-gas '100 Tgas' attached-deposit '1 yoctoNEAR' \
  sign-as <seller>.testnet network-config testnet sign-with-keychain send
```

`msg` must be exactly `{"price":"<yoctoNEAR as a decimal string>"}` (unknown fields are
rejected). The price must be `> 0` and `<= 10,000 NEAR` (`MAX_PRICE_YOCTO`). The token goes
straight back to the seller (the market returns `true`) when the message is invalid, the
price is out of range, listings are paused, the seller already has 20 open listings
(`MAX_LISTINGS_PER_SELLER`), or the market already has 1,000 open listings
(`MAX_LISTINGS_TOTAL`). The seller is `previous_owner_id`.

## Buy and failure handling

1. `buy` checks the deposit, removes the listing, and calls `nft_transfer` on the Snacc
   contract (1 yoctoNEAR, paid by the market). A second `buy`, `delist`, or `update_price`
   in the same window fails with `ERR_LISTING_NOT_FOUND`.
2. Transfer succeeded → the seller gets `price`, the buyer gets `deposit − price` back,
   `sold` is emitted.
3. Transfer failed (for example the strain died while listed, or it was adopted by someone
   else) → the buyer gets the **full deposit** back and the seller gets nothing. The market
   then reads `nft_token(token_id)`: if the market still owns the token the listing is
   restored as it was; if not, it stays removed and `delisted` (`reason:
   "token_left_market"`) is emitted. If that view call itself fails, the listing is
   restored (a stale listing can only make a later buy fail with a full refund).

A failed `delist` transfer goes through the same owner re-check. Listed strains keep living
on the Snacc contract (their vitality keeps decaying); check a strain with `nft_token`
before buying.

## Events (NEP-297)

`EVENT_JSON:{"standard":"snacc_market","version":"1.0.0","event":"<event>","data":[{...}]}`

| `event` | `data[0]` |
| --- | --- |
| `listed` | `token_id`, `seller_id`, `price`, `listed_at` |
| `sold` | `token_id`, `seller_id`, `buyer_id`, `price` |
| `delisted` | `token_id`, `seller_id`, `reason` (`"seller"` or `"token_left_market"`) |
| `price_updated` | `token_id`, `seller_id`, `old_price`, `new_price` |

## Storage

The market pays the storage of every listing from **its own balance**; sellers attach
nothing. Measured in the unit test `list_records_listing_and_emits_event`: 242 bytes for a
seller's first listing and 179 bytes for the next (short account names; longer names
cost more), i.e. about 0.002–0.004 NEAR per listing at 1 NEAR per 100 kB. The caps
(20 per seller, 1,000 total) bound it to a few NEAR at most. Storage is released when a
listing is sold or delisted. If the market balance cannot cover a new listing, that
`nft_on_transfer` fails and the Snacc contract returns the token to the seller.

## Build and test

```bash
cargo test -p snacc-market
(cd market && cargo near build non-reproducible-wasm)   # → target/near/snacc_market/snacc_market.wasm
```

## Known limits

- Payments are plain NEAR transfers. If a seller account is deleted before a sale settles,
  NEAR returns to the market account and stays there (there is no withdrawal method).
- A strain that died while listed cannot be transferred by the Snacc contract at all, so
  its listing stays visible but cannot be bought or delisted; every buy attempt is
  refunded in full (only gas is spent).
- Not audited. Unit tests only (no sandbox or testnet run in this change).

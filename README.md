# contracts — M1 `snacc-core` & M2 `snacc-svg`

A single atomic Rust contract, **`snacc-v001.near`** (NEP-141 `$SNACC`, live NEP-171/177 SVG, 9 NEP-245 molecules, Chimera Splicing, Cell Lysis, zero-trust TEE verification via `env::ed25519_verify`).

**Spec:** `snacc-vault/04 Arsitektur/Snacc - Spec Antarmuka Kontrak.md` · **Backlog:** `VC-001..010`, `DK-001..086` ([[Snacc - Backlog Master]]).

## Build & test

```bash
cargo check
cargo test                                               # whole workspace: snacc + market
(cd snacc && cargo near build non-reproducible-wasm)     # → target/near/snacc/snacc.wasm (after the toolchain target is installed)
(cd market && cargo near build non-reproducible-wasm)    # → target/near/snacc_market/snacc_market.wasm (EN-126 testnet market)
```

Pinned toolchain: `rustc 1.99.1`, `near-sdk =5.29.1` (the Kamus Angka Resmi).

The workspace has two crates: `snacc` (the game contract) and `market` (`snacc-market`, a
testnet-only marketplace for strains, 0% fee — see [`market/README.md`](market/README.md);
testnet NEAR and testnet `$SNACC` have no money value).

## Module map

| File | Module | Contents |
| --- | --- | --- |
| `snacc/src/lib.rs` | `M1` | Entrypoint, state D1–D8, methods F1–F6 |
| `snacc/src/svg.rs` | `M2` | Renders the 1,734-byte `NEP-177` SVG (8 indicators) |
| `snacc/src/events.rs` | `M1` | The `NEP-297` emitter (`EVENT_JSON:`) |
| `market/src/lib.rs` | `EN-126` | `snacc-market`: listings via NEP-171 `nft_transfer_call`, `buy` / `delist` / `update_price` |

## NEP-171 core transfer

`nft_transfer(receiver_id, token_id, approval_id, memo)` and
`nft_transfer_call(receiver_id, token_id, approval_id, memo, msg)` both need exactly
1 yoctoNEAR and share one transfer core (owner check, no dead cells, colony index,
`state_nonce`). `nft_transfer_call` then calls
`receiver_id.nft_on_transfer(sender_id, previous_owner_id, token_id, msg)` with all prepaid
gas except 50 TGas, and the private `nft_resolve_transfer` callback returns the token to the
previous owner when the receiver answers `true`, fails, or answers something that is not a
bool — but only while the receiver still holds the token and the cell is alive. It resolves
to `false` only when the token was returned. `approval_id` is a number or `null` and is
ignored (no NEP-178 approvals).

## Standard token events (`NEP-297` wrapper)

| `standard` | `event` | `data[0]` shape | When |
| --- | --- | --- | --- |
| `nep141` | `ft_mint` | `{"owner_id":"…","amount":"…","memo":"incubate_strain"}` | New supply is born: `incubate_strain` with an attached NEAR stake (the only mint path — `amount` is a string per the NEP-141 standard) |
| `nep141` | `ft_transfer` | `{"old_owner_id":"…","new_owner_id":"…","amount":…,"memo":…}` | `ft_transfer` / `ft_transfer_call` between accounts |
| `nep141` | `ft_burn` | `{"owner_id":"…","amount":…}` | `ft_burn` |
| `nep171` | `nft_transfer` | `{"old_owner_id":"…","new_owner_id":"…","token_ids":[…],…}` | `nft_transfer` / `nft_transfer_call`, and again (receiver → previous owner, `memo: "nft_transfer_call rejected"`) when `nft_resolve_transfer` returns the token |

Invariant (`I-01`): Σ(`ft_mint`) − Σ(`ft_burn`) == Δ`ft_total_supply`
(tested in `snacc/src/qa_nep_tests.rs`, `TC-NEP-09..12`).

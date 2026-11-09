# contracts — M1 `snacc-core` & M2 `snacc-svg`

A single atomic Rust contract, **`snacc-v001.near`** (NEP-141 `$SNACC`, live NEP-171/177 SVG, 9 NEP-245 molecules, Chimera Splicing, Cell Lysis, zero-trust TEE verification via `env::ed25519_verify`).

**Spec:** `snacc-vault/04 Arsitektur/Snacc - Spec Antarmuka Kontrak.md` · **Backlog:** `VC-001..010`, `DK-001..086` ([[Snacc - Backlog Master]]).

## Build & test

```bash
cargo check
cargo test
cargo near build   # wasm32-unknown-unknown, after the toolchain target is installed
```

Pinned toolchain: `rustc 1.99.1`, `near-sdk =5.29.1` (the Kamus Angka Resmi).

## Module map

| File | Module | Contents |
| --- | --- | --- |
| `snacc/src/lib.rs` | `M1` | Entrypoint, state D1–D8, methods F1–F6 |
| `snacc/src/svg.rs` | `M2` | Renders the 1,734-byte `NEP-177` SVG (8 indicators) |
| `snacc/src/events.rs` | `M1` | The `NEP-297` emitter (`EVENT_JSON:`) |

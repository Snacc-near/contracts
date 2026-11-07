# contracts — M1 `snacc-core` & M2 `snacc-svg`

Kontrak Rust tunggal atomik **`snacc-v001.near`** (NEP-141 `$SNACC`, NEP-171/177 SVG live, NEP-245 9 molekul, Chimera Splicing, Cell Lysis, verifikasi TEE zero-trust `env::ed25519_verify`).

**Spec:** `snacc-vault/04 Arsitektur/Snacc - Spec Antarmuka Kontrak.md` · **Backlog:** `VC-001..010`, `DK-001..086` ([[Snacc - Backlog Master]]).

## Build & test

```bash
cargo check
cargo test
cargo near build   # wasm32-unknown-unknown, setelah toolchain target terpasang
```

Toolchain terpin: `rustc 1.99.1`, `near-sdk =5.29.1` (Kamus Angka Resmi).

## Peta modul

| File | Modul | Isi |
| --- | --- | --- |
| `snacc/src/lib.rs` | `M1` | Entrypoint, state D1–D8, method F1–F6 |
| `snacc/src/svg.rs` | `M2` | Render `NEP-177` SVG 1.734 byte (8 indikator) |
| `snacc/src/events.rs` | `M1` | Emitter `NEP-297` (`EVENT_JSON:`) |

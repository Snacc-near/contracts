//! Emitter event standar `NEP-297` (`EVENT_JSON:`) untuk `F1–F6` (`VC-010`).
//!
//! Event wajib (spec): `strain_divided`, `transmuted`, `atp_minted`,
//! `nutrient_burst_released`, `zone_changed`, `strains_spliced`,
//! `strain_liquidated`, `strain_adopted`, `aether_burned_for_dividend`,
//! `dividend_claimed`, `epoch_dividend_closed`.

/// Cetak log `EVENT_JSON:` — placeholder sampai `VC-010`.
pub fn emit_event_json(_event_json: &str) {
    // TODO(VC-010): near_sdk::log_str(&format!("EVENT_JSON:{}", ...))
}

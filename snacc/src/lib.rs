//! Snacc — kontrak inti `snacc-v001.near` (M1 `snacc-core` + M2 `snacc-svg`).
//!
//! Empat standar dalam satu kontrak atomik:
//! `NEP-141` ($SNACC) · `NEP-171`/`NEP-177` (Homunculus Strain + SVG live) ·
//! `NEP-245` (9 molekul) · mekanik F1–F7 (M&A, Cell Lysis, TEE zero-trust).
//!
//! Peta: [[Snacc - Katalog Fitur]] · [[Snacc - Spec Antarmuka Kontrak]] ·
//! [[Snacc - Model Data]] (D1–D9) · [[Snacc - Backlog Master]].

pub mod events;
pub mod state;
pub mod svg;

use near_sdk::store::{LookupMap, LookupSet, Vector};
use near_sdk::{AccountId, near_bindgen, PanicOnDefault};
use state::{
    AlkemisProfile, MacroState, MnaLogEntry, StrainId, StrainVessel, TeeAttestation, TokenId,
};

/// State kontrak — `D1–D8` (`D9` = event `NEP-297`, tanpa state). `VC-002`.
#[near_bindgen]
#[derive(PanicOnDefault)]
pub struct SnaccContract {
    /// `D1` Registry Alkemis.
    pub alchemists: LookupMap<AccountId, AlkemisProfile>,
    /// `D2` Ledger Strain & neraca virtual/sub-account.
    pub strains: LookupMap<StrainId, StrainVessel>,
    /// `D3` Inventori multi-token `NEP-245` per Strain.
    pub inventories: LookupMap<(StrainId, TokenId), u128>,
    /// `D5` Registry bukti attestation TEE (`ADR-21`).
    pub tee_attestations: LookupMap<String, TeeAttestation>,
    /// `D5` Anti-replay: `chat_id` yang sudah dipakai (`used_tee_chat_ids`).
    pub used_tee_chat_ids: LookupSet<String>,
    /// `D6` Kas `$SNACC` per Strain (neraca `vault_subaccount`).
    pub snacc_balances: LookupMap<StrainId, u128>,
    /// `D6` `Rule-Bound Treasury` — tanpa fungsi penarikan admin (`I-05`).
    pub treasury: u128,
    /// `D7` Parameter & state termostat moneter (`CrucibleTemp`, burst, dividen).
    pub macro_state: MacroState,
    /// `D8` Log M&A & likuidasi.
    pub mna_logs: Vector<MnaLogEntry>,
}

#[near_bindgen]
impl SnaccContract {
    #[init]
    pub fn new() -> Self {
        Self {
            alchemists: LookupMap::new(b"a".to_vec()),
            strains: LookupMap::new(b"s".to_vec()),
            inventories: LookupMap::new(b"i".to_vec()),
            tee_attestations: LookupMap::new(b"t".to_vec()),
            used_tee_chat_ids: LookupSet::new(b"u".to_vec()),
            snacc_balances: LookupMap::new(b"b".to_vec()),
            treasury: 0,
            macro_state: MacroState::default(),
            mna_logs: Vector::new(b"m".to_vec()),
        }
    }

    /// Penanda versi protokol — dipakai smoke test & verifikasi deploy.
    pub fn protocol_version(&self) -> &'static str {
        "snacc-v001"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_scaffold() {
        let c = SnaccContract::new();
        assert_eq!(c.protocol_version(), "snacc-v001");
        assert_eq!(c.treasury, 0);
        assert_eq!(c.macro_state.current_epoch, 0);
    }
}

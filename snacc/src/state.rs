//! State `D1–D8` — `VC-002` ([[Snacc - Model Data]]).
//!
//! Nama field/method disalin persis dari spec: [[Snacc - Spec Antarmuka Kontrak]],
//! `ADR-22/25/26/29`, dan **Kamus Angka Resmi** ([[Snacc - Model Data]]).
//! Semua struct inti memakai `#[near(serializers = [borsh, json])]` (DoD `VC-002`).

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::{AccountId, near};

pub type StrainId = u64;
pub type TokenId = String; // `NEP-245`, mis. `mt:spore-ignis`
pub type Epoch = u64;

/// Konstanta kanonik (Kamus Angka Resmi — jangan pakai angka lain).
pub mod constants {
    /// Guardrail mutasi TEE (`ADR-21`): `maxShiftBps = ±1500`.
    pub const MAX_SHIFT_BPS: u16 = 1500;
    /// Calcination Fever saat `CrucibleTemp > 1400` mK.
    pub const FEVER_THRESHOLD_MK: u64 = 1400;
    /// Hypothermia Rebate saat `CrucibleTemp < 600` mK.
    pub const HYPOTHERMIA_THRESHOLD_MK: u64 = 600;
    /// `ft_burn 350 bps` (3,5%) pada biaya katalis saat Fever.
    pub const FEVER_BURN_BPS: u16 = 350;
    /// Subsidi `200 bps` (2,0%) dari treasury saat Hypothermia.
    pub const REBATE_BPS: u16 = 200;
    /// Cap drain treasury `500 bps`/epoch (5% Pantry/epoch, invariant `I-04`).
    pub const MAX_EPOCH_TREASURY_DRAIN_BPS: u16 = 500;
    /// Peluruhan volume kontinu (`VOLUME_DECAY_PER_SEC = 50`).
    pub const VOLUME_DECAY_PER_SEC: u64 = 50;
    /// Bounty Cell Lysis: `10%` likuidator / `90%` `Rule-Bound Treasury`.
    pub const LIQUIDATOR_BOUNTY_BPS: u16 = 1000;
    /// Pajak merger `2.5%` ke Treasury (`ADR-25`).
    pub const MERGER_TAX_BPS: u16 = 250;
    /// Premi hostile takeover: `book value + 15%` (`ADR-26` Tier 2).
    pub const HOSTILE_PREMIUM_BPS: u16 = 1500;
    /// Pagar koloni (`EN-08`).
    pub const MAX_COLONY_SIZE: u8 = 5;
    /// Pagar populasi (`EN-08`).
    pub const MAX_GENERATION: u8 = 8;
    /// Batas atas diskon Nutrient Burst (`EN-01`, spec `0..2000`).
    pub const MAX_BURST_BPS: u16 = 2000;
}

/// Status solvabilitas — tangga 3 tingkat (`ADR-26`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum StrainStatus {
    /// Tier 1: `effective_vitality > 2000` (>20%) — terlindungi penuh.
    Active,
    /// Tier 2: `1..=2000` — jendela Chapter 11 (`hostile_takeover_splice` terbuka).
    Distressed,
    /// Tier 3: `0` — Cell Lysis (`harvest_starving_strain` terbuka).
    Liquidated,
}

/// Slot organel (`EN-07`): MVP 3 slot — alokasi `$ATP` 1× per epoch.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OrganelleSlots {
    /// Mitokondria: menaikkan `atp_minted` (EN-06).
    pub mitokondria: u8,
    /// Nukleus: menaikkan ketahanan BMR.
    pub nukleus: u8,
    /// Lisosom: memperbesar Nutrient Burst saat `snack`.
    pub lisosom: u8,
}

/// Profil Alkemis (`D1`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct AlkemisProfile {
    pub account_id: AccountId,
    /// Detik pertama kali onboarding (`incubate_strain`).
    pub joined_sec: u64,
    /// Jumlah Strain yang dimiliki (cache `get_colony`, `EN-08`).
    pub strain_count: u8,
}

/// `StrainVessel` (`D2`) — neraca berdaulat satu `Homunculus Strain`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct StrainVessel {
    pub strain_id: StrainId,
    pub owner_id: AccountId,
    /// Keyless sub-account (`"keys": []`) penyimpan aset — `ADR-02`, `ADR-29`.
    pub vault_subaccount: AccountId,
    /// Vitalitas (kanonik tangga: `0`, `1..=2000`, `>2000`).
    pub vitality: u64,
    /// `BMR` — peluruhan vitalitas per jam (`ADR-22`).
    pub bmr_per_hour: u64,
    /// Timestamp (detik) pemberian makan terakhir.
    pub last_fed_sec: u64,
    /// Bitmask Coasean (`ADR-25`): `ROLE_EXTRACTOR=0b001`, `ROLE_FERMENTER=0b010`, `ROLE_SYNTHESIZER=0b100`.
    pub role_mask: u8,
    /// Monotonic nonce anti-drain (`ADR-29`).
    pub state_nonce: u64,
    /// Wajib `0` saat mutasi; dikunci saat cross-contract berjalan (`ADR-29`).
    pub pending_callbacks: u8,
    /// Energi internal (`EN-06`) — **non-transferable, bukan token**, tanpa `total_supply`.
    pub atp_balance: u128,
    /// Generasi hasil `splice_strains`/`divide_strain` (`EN-08`, maks `MAX_GENERATION`).
    pub generation: u8,
    /// Set organel (`EN-07`); diwariskan saat merger = endosimbiosis (`EN-120`).
    pub organel: OrganelleSlots,
    pub status: StrainStatus,
}

impl StrainVessel {
    /// `effective_vitality = vitality - elapsed_hours * bmr_per_hour` (Lazy BMR).
    pub fn effective_vitality(&self, now_sec: u64) -> u64 {
        let elapsed_hours = now_sec.saturating_sub(self.last_fed_sec) / 3600;
        self.vitality
            .saturating_sub(elapsed_hours.saturating_mul(self.bmr_per_hour))
    }

    /// Pengunci mutasi saldo/inventori (`ADR-29`) — tolak bila ada async berjalan
    /// atau nonce berubah (serangan `ERC-6551` seller-drain).
    pub fn assert_atomic_ready(&self, expected_nonce: u64) {
        assert_eq!(
            self.pending_callbacks, 0,
            "ERR_PENDING_ASYNC_CALLBACK_LOCKED"
        );
        assert_eq!(
            self.state_nonce, expected_nonce,
            "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED"
        );
    }

    /// Setiap mutasi saldo `$SNACC`/inventori `NEP-245` menaikkan nonce (`ADR-29`).
    pub fn record_mutation(&mut self) {
        self.state_nonce = self.state_nonce.saturating_add(1);
    }
}

/// Parameter termostat moneter (`D7`) — semua nilai mengikuti Kamus Angka Resmi.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct TermostatParams {
    pub fever_threshold_mk: u64,
    pub hypothermia_threshold_mk: u64,
    pub fever_burn_bps: u16,
    pub rebate_bps: u16,
    pub max_epoch_treasury_drain_bps: u16,
    pub volume_decay_per_sec: u64,
    pub max_shift_bps: u16,
}

impl Default for TermostatParams {
    fn default() -> Self {
        use constants::*;
        Self {
            fever_threshold_mk: FEVER_THRESHOLD_MK,
            hypothermia_threshold_mk: HYPOTHERMIA_THRESHOLD_MK,
            fever_burn_bps: FEVER_BURN_BPS,
            rebate_bps: REBATE_BPS,
            max_epoch_treasury_drain_bps: MAX_EPOCH_TREASURY_DRAIN_BPS,
            volume_decay_per_sec: VOLUME_DECAY_PER_SEC,
            max_shift_bps: MAX_SHIFT_BPS,
        }
    }
}

/// `MacroState` (`D7`) — state makro `CrucibleTemp` + Nutrient Burst + dividen.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct MacroState {
    pub current_epoch: Epoch,
    /// Timestamp blok (ms) pembaruan terakhir — **dari `env::block_timestamp_ms()`** (`V-02`).
    pub last_update_ms: u64,
    /// `V_epoch` — volume aktivitas ter-decay untuk `CrucibleTemp = 1000 × V / M_supply`.
    pub volume_epoch: u128,
    /// Diskon feed aktif (`EN-01`, `0..=MAX_BURST_BPS`).
    pub nutrient_burst_bps: u16,
    /// Epoch berakhirnya burst (`EN-01`).
    pub nutrient_burst_until: Epoch,
    /// `industrial_dividend_pool` (`DK-033`) — akumulasi 50% fee non-burn.
    pub industrial_dividend_pool: u128,
    /// Inflow dividen epoch berjalan (`DK-033`).
    pub epoch_dividend_inflow: u128,
    /// Total `mt:elixir-aether` terbakar epoch berjalan (`DK-044`).
    pub epoch_aether_burned_total: u128,
    /// Epoch terakhir yang ditutup (`advance_epoch`, `DK-033`).
    pub last_closed_epoch: Epoch,
    pub params: TermostatParams,
}

impl Default for MacroState {
    fn default() -> Self {
        Self {
            current_epoch: 0,
            last_update_ms: 0,
            volume_epoch: 0,
            nutrient_burst_bps: 0,
            nutrient_burst_until: 0,
            industrial_dividend_pool: 0,
            epoch_dividend_inflow: 0,
            epoch_aether_burned_total: 0,
            last_closed_epoch: 0,
            params: TermostatParams::default(),
        }
    }
}

/// Bukti attestation TEE (`D5`) — disimpan dari `apply_tee_mutation_zero_trust` (`ADR-21`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct TeeAttestation {
    pub chat_id: String,
    pub model: String,
    pub req_sha256_hex: String,
    pub res_sha256_hex: String,
    /// Pubkey ed25519 enclave (hex) yang diverifikasi via `env::ed25519_verify`.
    pub signer_pubkey_hex: String,
}

/// Log M&A & Likuidasi (`D8`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct MnaLogEntry {
    pub timestamp_ms: u64,
    pub kind: MnaKind,
    pub actor_id: AccountId,
    pub target_id: StrainId,
    pub amount: u128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum MnaKind {
    /// `splice_strains` (`F5`).
    Splice,
    /// `hostile_takeover_splice` (`F5`, Tier 2).
    HostileTakeover,
    /// `harvest_starving_strain` (`F6`).
    Lysis,
    /// `adopt` (`EN-08`).
    Adopt,
    /// `divide_strain` (`EN-08`).
    Divide,
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::serde_json;

    fn sample_strain() -> StrainVessel {
        StrainVessel {
            strain_id: 1,
            owner_id: AccountId::new_unvalidated("alkemis.testnet".to_string()),
            vault_subaccount: AccountId::new_unvalidated("s-1.snacc-v001.near".to_string()),
            vitality: 2500,
            bmr_per_hour: 100,
            last_fed_sec: 3600,
            role_mask: 0b001,
            state_nonce: 7,
            pending_callbacks: 0,
            atp_balance: 42,
            generation: 0,
            organel: OrganelleSlots { mitokondria: 1, nukleus: 0, lisosom: 0 },
            status: StrainStatus::Active,
        }
    }

    #[test]
    fn vc002_borsh_json_roundtrip() {
        let v = sample_strain();
        let bytes = near_sdk::borsh::to_vec(&v).unwrap();
        let back: StrainVessel = near_sdk::borsh::from_slice(&bytes).unwrap();
        assert_eq!(v, back);

        let json = serde_json::to_string(&v).unwrap();
        let back: StrainVessel = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);

        let m = MacroState::default();
        let json = serde_json::to_string(&m).unwrap();
        let back: MacroState = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn vc002_kamus_angka_terkunci() {
        let p = TermostatParams::default();
        assert_eq!(p.fever_threshold_mk, 1400);
        assert_eq!(p.hypothermia_threshold_mk, 600);
        assert_eq!(p.fever_burn_bps, 350);
        assert_eq!(p.rebate_bps, 200);
        assert_eq!(p.max_epoch_treasury_drain_bps, 500);
        assert_eq!(p.volume_decay_per_sec, 50);
        assert_eq!(p.max_shift_bps, 1500);
        assert_eq!(constants::LIQUIDATOR_BOUNTY_BPS, 1000);
        assert_eq!(constants::MERGER_TAX_BPS, 250);
        assert_eq!(constants::HOSTILE_PREMIUM_BPS, 1500);
        assert_eq!(constants::MAX_COLONY_SIZE, 5);
        assert_eq!(constants::MAX_GENERATION, 8);
    }

    #[test]
    fn vc002_lazy_bmr_effective_vitality() {
        let v = sample_strain(); // last_fed 3600, bmr 100/jam
        assert_eq!(v.effective_vitality(3600), 2500);
        assert_eq!(v.effective_vitality(3600 + 5 * 3600), 2000); // 5 jam × 100
        assert_eq!(v.effective_vitality(3600 + 100 * 3600), 0); // saturasi ke 0
    }

    #[test]
    fn vc002_record_mutation_naikkan_nonce() {
        let mut v = sample_strain();
        v.record_mutation();
        assert_eq!(v.state_nonce, 8);
    }

    #[test]
    #[should_panic(expected = "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED")]
    fn vc002_nonce_mismatch_ditolak() {
        let v = sample_strain();
        v.assert_atomic_ready(99);
    }

    #[test]
    #[should_panic(expected = "ERR_PENDING_ASYNC_CALLBACK_LOCKED")]
    fn vc002_pending_callback_mengunci() {
        let mut v = sample_strain();
        v.pending_callbacks = 1;
        v.assert_atomic_ready(7);
    }
}

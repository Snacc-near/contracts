//! State `D1–D8` — `VC-002` ([[Snacc - Model Data]]).
//!
//! Nama field/method disalin persis dari spec: [[Snacc - Spec Antarmuka Kontrak]],
//! `ADR-22/25/26/29`, dan **Kamus Angka Resmi** ([[Snacc - Model Data]]).
//! Semua struct inti memakai `#[near(serializers = [borsh, json])]` (DoD `VC-002`).

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
    /// `F1` — vitalitas awal Strain Genesis = `8500` (`85.00%`).
    pub const INITIAL_VITALITY: u64 = 8500;
    /// `F1` — efisiensi awal = `10000` bps (`1.00x`).
    pub const INITIAL_EFFICIENCY_BPS: u16 = 10000;
    /// `F1` — `bmr_per_hour` awal per kasta (`Katalog Molekul`).
    pub const BMR_EXTRACTOR_PER_HOUR: u64 = 100;
    pub const BMR_FERMENTER_PER_HOUR: u64 = 120;
    pub const BMR_SYNTHESIZER_PER_HOUR: u64 = 150;
    /// `ADR-25` / `I-08` — bitmask peran Coasean (wajib tunggal saat Genesis).
    pub const ROLE_EXTRACTOR: u8 = 0b001;
    pub const ROLE_FERMENTER: u8 = 0b010;
    pub const ROLE_SYNTHESIZER: u8 = 0b100;
    /// `F1` — `20%` fee inkubasi ke `Rule-Bound Treasury`; `80%` modal kerja Strain.
    pub const STAKE_TREASURY_BPS: u128 = 2000;
    pub const BPS_DENOMINATOR: u128 = 10000;
    /// `F1` — deposit minimum `materialize_subaccount` = `0.02 NEAR` (20 milliNEAR).
    pub const MIN_MATERIALIZE_DEPOSIT_MILLINEAR: u128 = 20;
    /// `F1` — jumlah spora genesis yang dicetak per inkubasi (`Spec F1`, belum ternominalisasi).
    pub const GENESIS_SPORE_AMOUNT: u64 = 1;
    /// `F1` — format sub-account kanonik `s-<id>.snacc-v001.near`.
    pub const VAULT_SUBACCOUNT_PREFIX: &str = "s-";
    /// `F1` — kode error: strain sudah pernah diinkubasi.
    pub const ERR_STRAIN_ALREADY_INCUBATED: &str = "ERR_STRAIN_ALREADY_INCUBATED";
    /// `F1` — kode error: role bitmask bukan tunggal bit.
    pub const ERR_ROLE_MASK_NOT_SINGLE: &str = "ERR_ROLE_MASK_NOT_SINGLE";
    /// `F1` — kode error: deposit `materialize_subaccount` kurang dari 0.02 NEAR.
    pub const ERR_INSUFFICIENT_DEPOSIT: &str = "ERR_INSUFFICIENT_DEPOSIT";
    /// `F1` — kode error: pemanggil bukan owner strain.
    pub const ERR_NOT_OWNED_BY_CALLEE: &str = "ERR_NOT_OWNED_BY_CALLEE";
    /// `F1` — kode error: `initial_snacc_stake` nominal != attached deposit.
    pub const ERR_STAKE_DEPOSIT_MISMATCH: &str = "ERR_STAKE_DEPOSIT_MISMATCH";
    /// `F1` — kode error: strain tidak ditemukan.
    pub const ERR_STRAIN_NOT_FOUND: &str = "ERR_STRAIN_NOT_FOUND";
    /// `F1` — token spora genesis `NEP-245` (`Katalog Molekul`, `DFD P1.3`).
    pub const GENESIS_SPORE_TOKEN_ID: &str = "mt:spore-ignis";
    /// `ADR-02` — format sub-account kanonik `s-<id>.snacc-v001.near`.
    pub const VAULT_SUBACCOUNT_SUFFIX: &str = ".snacc-v001.near";
    /// `F2` — pemulihan metabolik per transmutasi sukses: `vitality +500`.
    pub const METABOLIC_RECOVERY_VITALITY: u64 = 500;
    /// `I-02` — batas atas vitalitas (`0..=10000`).
    pub const VITALITY_CAP: u64 = 10000;
    /// `F2` — id token `NEP-245` Tahap 1 (spora).
    pub const SPORE_TOKEN_IDS: [&str; 3] = ["mt:spore-ignis", "mt:spore-cryo", "mt:spore-lumin"];
    /// `F2` — id token Tahap 2 (enzim, resep Katalog Molekul).
    pub const ENZYME_AMALGAM: &str = "mt:enzyme-amalgam";
    pub const ENZYME_REDACTASE: &str = "mt:enzyme-redactase";
    pub const ENZYME_SYNTHASE: &str = "mt:enzyme-synthase";
    /// `F2` — id token Tahap 3 (eliksir).
    pub const ELIXIR_HOMUNCULUS: &str = "mt:elixir-homunculus";
    pub const ELIXIR_HOMEOSTASIS: &str = "mt:elixir-homeostasis";
    pub const ELIXIR_AETHER: &str = "mt:elixir-aether";
    /// `F2` — kode error rantai pasok.
    pub const ERR_STRAIN_STARVING: &str = "ERR_STRAIN_STARVING";
    pub const ERR_UNKNOWN_TOKEN: &str = "ERR_UNKNOWN_TOKEN";
    pub const ERR_ROLE_NOT_ALLOWED: &str = "ERR_ROLE_NOT_ALLOWED";
    pub const ERR_INSUFFICIENT_BALANCE: &str = "ERR_INSUFFICIENT_BALANCE";
    pub const ERR_RECIPE_NOT_MET: &str = "ERR_RECIPE_NOT_MET";
    pub const ERR_REQUIRES_TEE_ATTESTATION: &str = "ERR_REQUIRES_TEE_ATTESTATION";
    /// `F3` — model TEE verifikasi (Kamus Angka Resmi; pesan signed: `<model>:<req_sha256>:<res_sha256>`, `S2.6`).
    pub const TEE_MODEL_ID: &str = "z-ai/glm-5.3-flash";
    /// `I-03` — guardrail mutasi TEE.
    pub const TEE_SHIFT_LIMIT_BPS: i64 = 1500;
    pub const TEE_VITALITY_BONUS_CAP: u64 = 500;
    /// `F3` — kode error TEE (nama mismatch diambil dari red-team spike `S1.9`).
    pub const ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH: &str =
        "ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH";
    pub const ERR_COMMITMENT_MISMATCH: &str = "ERR_COMMITMENT_MISMATCH";
    pub const ERR_COMMITMENT_ALREADY_PENDING: &str = "ERR_COMMITMENT_ALREADY_PENDING";
    pub const ERR_SIGNER_NOT_REGISTERED: &str = "ERR_SIGNER_NOT_REGISTERED";
    pub const ERR_TEE_REPLAY: &str = "ERR_TEE_REPLAY";
    pub const ERR_PAYLOAD_STRAIN_MISMATCH: &str = "ERR_PAYLOAD_STRAIN_MISMATCH";
    pub const ERR_GUARDRAIL_SHIFT: &str = "ERR_GUARDRAIL_SHIFT";
    pub const ERR_GUARDRAIL_VITALITY: &str = "ERR_GUARDRAIL_VITALITY";
    pub const ERR_BAD_HEX: &str = "ERR_BAD_HEX";
    pub const ERR_BAD_SIGNATURE_LEN: &str = "ERR_BAD_SIGNATURE_LEN";
    pub const ERR_NOT_CONTRACT_OWNER: &str = "ERR_NOT_CONTRACT_OWNER";
    /// `F5` — kode error merger (nama `ERR_NOT_SAME_OWNER` dari spec F5).
    pub const ERR_NOT_SAME_OWNER: &str = "ERR_NOT_SAME_OWNER";
    pub const ERR_SAME_STRAIN: &str = "ERR_SAME_STRAIN";
    pub const ERR_STRAIN_ALREADY_SPLICED: &str = "ERR_STRAIN_ALREADY_SPLICED";
    /// `F2`/`F5` — sembilan id molekul `NEP-245` (Katalog Molekul).
    pub const ALL_TOKEN_IDS: [&str; 9] = [
        "mt:spore-ignis",
        "mt:spore-cryo",
        "mt:spore-lumin",
        "mt:enzyme-amalgam",
        "mt:enzyme-redactase",
        "mt:enzyme-synthase",
        "mt:elixir-homunculus",
        "mt:elixir-homeostasis",
        "mt:elixir-aether",
    ];
    /// `F6` — kode error Cell Lysis (nama dari spec F6 & Alur POV).
    pub const ERR_STRAIN_NOT_INSOLVENT: &str = "ERR_STRAIN_NOT_INSOLVENT";
    pub const ERR_ALREADY_LIQUIDATED: &str = "ERR_ALREADY_LIQUIDATED";
    /// `N141` — kode error: penerima `ft_transfer` belum terdaftar storage.
    pub const ERR_ACCOUNT_NOT_REGISTERED: &str = "ERR_ACCOUNT_NOT_REGISTERED";
    /// `N141` — kode error: `ft_transfer`/`ft_transfer_call` tanpa tepat 1 yoctoNEAR.
    pub const ERR_YOCTO_REQUIRED: &str = "ERR_YOCTO_REQUIRED";
    /// `DK-033/044` — kode error (nama persis dari Spec Dividend Pool §6).
    pub const ERR_NOT_STRAIN_OWNER: &str = "ERR_NOT_STRAIN_OWNER";
    pub const ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE: &str =
        "ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE";
    pub const ERR_INSUFFICIENT_AETHER: &str = "ERR_INSUFFICIENT_AETHER";
    pub const ERR_EPOCH_NOT_CLOSED: &str = "ERR_EPOCH_NOT_CLOSED";
    pub const ERR_DIVIDEND_ALREADY_CLAIMED: &str = "ERR_DIVIDEND_ALREADY_CLAIMED";
    pub const ERR_DIVIDEND_CLAIM_EXPIRED: &str = "ERR_DIVIDEND_CLAIM_EXPIRED";
    pub const ERR_DIVIDEND_POOL_EMPTY: &str = "ERR_DIVIDEND_POOL_EMPTY";
    /// `DK-033/044` — konstanta terdokumentasi (Spec Dividend Pool §3).
    /// `DIVIDEND_INFLOW_BPS = 5000` (50% dari fee non-burn, `ADR-28`).
    pub const DIVIDEND_INFLOW_BPS: u128 = 5000;
    /// `DIVIDEND_RATE_SCALE = 10^18` — hindari pembulatan merugikan pemain kecil.
    pub const DIVIDEND_RATE_SCALE: u128 = 1_000_000_000_000_000_000;
    /// `MAX_DIVIDEND_CLAIM_EPOCHS = 8` — batas klaim mundur, selaras `MAX_GENERATION`.
    pub const MAX_DIVIDEND_CLAIM_EPOCHS: u64 = 8;
    /// `MIN_AETHER_BURN = 1` — cegah spam 0-amount (keputusan tercatat: spec tidak ternominalisasi).
    pub const MIN_AETHER_BURN: u128 = 1;
    /// `EN-01` — `BASE_BURST_BPS = 200` (2%), selaras `REBATE_BPS` (Spec Enhancement §3.3).
    pub const BASE_BURST_BPS: u16 = 200;
    /// `EN-01` — `BURST_DURATION_EPOCHS = 1`.
    pub const BURST_DURATION_EPOCHS: u64 = 1;
    /// `N141` — deposit storage minimum MVP = 5 milliNEAR (keputusan tercatat).
    pub const STORAGE_DEPOSIT_MILLINEAR: u128 = 5;
    /// `F5` Jalur 2 — kode error: target tidak berada di Tier 2 Chapter 11 Distressed
    /// (`1 <= effective_vitality <= 2000`). Nama keputusan tercatat (spec tidak menamai).
    pub const ERR_NOT_DISTRESSED: &str = "ERR_NOT_DISTRESSED";
    /// `F5` Jalur 2 — pembayaran buyout = `book_value × (10000 + HOSTILE_PREMIUM_BPS) / 10000`
    /// (`ADR-26`: Book Value + 15% Restructuring Premium).
    pub const HOSTILE_SELLER_BPS: u128 = 9500; // 95% pembayaran ke pemilik lama
    pub const HOSTILE_TREASURY_BPS: u128 = 500; // 5% ke Rule-Bound Treasury
    /// `F5` Jalur 2 — `floor_price` molekul belum ternominalisasi di spec → keputusan
    /// tercatat: 1 unit `$SNACC` per unit molekul (transparan, mudah diganti).
    pub const MOLECULE_FLOOR_PRICE: u128 = 1;
    /// `EN-02`/`EN-105` — Great Feast (keputusan tercatat: target & band tidak
    /// ternominalisasi di spec): target volume feed/epoch = 5% dari total supply,
    /// band Goldilocks = ±20% dari target.
    pub const FEAST_TARGET_BPS: u128 = 500;
    pub const FEAST_BAND_BPS: u128 = 2000;
    /// `EN-108` — dormansi: tarif upkeep feed turun ke `UPKEEP_MIN_BPS` (`50` bps,
    /// dari Peta Koneksi D7) saat `decayed_volume < ambang`. Ambang = keputusan
    /// tercatat: 1% dari total supply (`DORMANCY_THRESHOLD_BPS`).
    pub const UPKEEP_MIN_BPS: u128 = 50;
    pub const DORMANCY_THRESHOLD_BPS: u64 = 100;
    /// `EN-109` — rescue feeding (keputusan tercatat: linear; fee minimum `20%`
    /// saat `effective_vitality -> 0`, penuh `100%` di `effective_vitality = 10000`).
    pub const RESCUE_MIN_FEE_BPS: u128 = 2000;
    /// `EN-110` — matriks diet per kasta per zona (keputusan tercatat; arah dari
    /// `LC-14`: Fever → Extractor murah, Famine → Synthesizer murah; lainnya 0).
    pub const DIET_FEVER_EXTRACTOR_BPS: u16 = 2000;
    pub const DIET_FAMINE_SYNTHESIZER_BPS: u16 = 2000;
}

/// Helper macro: panic with a constant `ERR_*` error code.
/// `assert!`/`assert_eq!` require a string-literal format first, so we expose
/// `fail!(err_code)` = `panic!(err_code)`.
#[macro_export]
macro_rules! fail {
    ($err:expr) => {
        panic!($err)
    };
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
    /// `F5`: terserap habis oleh merger (`splice_strains`) — tidak dapat dipakai lagi.
    SplicedBurned,
}

/// Kasta peran industri `Homunculus Strain` (`NEP-171`, `I-08`).
/// Enum menjamin role tunggal saat Genesis; multi-bit hanya lewat `splice_strains`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrainRole {
    Extractor,
    Fermenter,
    Synthesizer,
}

impl StrainRole {
    /// Bitmask Coasean tunggal (`ADR-25`): `0b001` / `0b010` / `0b100`.
    pub const fn to_role_mask(self) -> u8 {
        match self {
            StrainRole::Extractor => constants::ROLE_EXTRACTOR,
            StrainRole::Fermenter => constants::ROLE_FERMENTER,
            StrainRole::Synthesizer => constants::ROLE_SYNTHESIZER,
        }
    }

    /// `bmr_per_hour` awal per kasta (`F1`, `Katalog Molekul`): `100` / `120` / `150`.
    pub const fn bmr_per_hour(self) -> u64 {
        match self {
            StrainRole::Extractor => constants::BMR_EXTRACTOR_PER_HOUR,
            StrainRole::Fermenter => constants::BMR_FERMENTER_PER_HOUR,
            StrainRole::Synthesizer => constants::BMR_SYNTHESIZER_PER_HOUR,
        }
    }
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
    /// Efisiensi konversi `NEP-245` (`F2`): `10000 bps = 1.00x`.
    pub efficiency_bps: u16,
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
    /// `F3` (`ADR-21`): komitmen `req_sha256_hex` Phase 1 (`commit_genome_experiment`).
    pub pending_commitment: Option<String>,
    /// `F3`: trait fenotip hasil mutasi TEE (payload `phenotype_trait`).
    pub phenotype_trait: Option<String>,
    /// Generasi hasil `splice_strains`/`divide_strain` (`EN-08`, maks `MAX_GENERATION`).
    pub generation: u8,
    /// Set organel (`EN-07`); diwariskan saat merger = endosimbiosis (`EN-120`).
    pub organel: OrganelleSlots,
    pub status: StrainStatus,
}

/// Tampilan baca-saja `get_strain` (`F1`) — mirror `StrainVessel` + `effective_vitality(now)`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct StrainVesselView {
    pub strain_id: StrainId,
    pub owner_id: AccountId,
    pub vault_subaccount: AccountId,
    pub vitality: u64,
    pub efficiency_bps: u16,
    pub bmr_per_hour: u64,
    pub last_fed_sec: u64,
    pub role_mask: u8,
    pub state_nonce: u64,
    pub pending_callbacks: u8,
    pub atp_balance: u128,
    pub pending_commitment: Option<String>,
    pub phenotype_trait: Option<String>,
    pub generation: u8,
    pub organel: OrganelleSlots,
    pub status: StrainStatus,
    /// Vitalitas efektif terhitung untuk `now` (Lazy BMR, `ADR-22`).
    pub effective_vitality: u64,
}

impl StrainVessel {
    /// Bangun view untuk `now_sec` (detik).
    pub fn to_view(&self, now_sec: u64) -> StrainVesselView {
        StrainVesselView {
            strain_id: self.strain_id,
            owner_id: self.owner_id.clone(),
            vault_subaccount: self.vault_subaccount.clone(),
            vitality: self.vitality,
            efficiency_bps: self.efficiency_bps,
            bmr_per_hour: self.bmr_per_hour,
            last_fed_sec: self.last_fed_sec,
            role_mask: self.role_mask,
            state_nonce: self.state_nonce,
            pending_callbacks: self.pending_callbacks,
            atp_balance: self.atp_balance,
            pending_commitment: self.pending_commitment.clone(),
            phenotype_trait: self.phenotype_trait.clone(),
            generation: self.generation,
            organel: self.organel.clone(),
            status: self.status.clone(),
            effective_vitality: self.effective_vitality(now_sec),
        }
    }

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

/// Zona makro (`EN-106`) — disimpan untuk deteksi transisi `zone_changed`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum MacroZone {
    #[default]
    Normal,
    Fever,
    Hypothermia,
}

impl MacroZone {
    pub fn as_str(&self) -> &'static str {
        match self {
            MacroZone::Normal => "NORMAL",
            MacroZone::Fever => "FEVER",
            MacroZone::Hypothermia => "HYPO",
        }
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
    /// `EN-108` — ambang dormansi (`%` supply × 100): dormansi saat
    /// `decayed_volume < supply × dormancy_threshold_bps / 10000`.
    pub dormancy_threshold_bps: u64,
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
            dormancy_threshold_bps: DORMANCY_THRESHOLD_BPS,
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
    /// Total suplai `$SNACC` beredar (`I-01`, bertambah saat `incubate_strain`, berkurang saat burn).
    pub total_snacc_supply: u128,
    /// Akumulasi `$SNACC` yang dibakar (`I-01`, Fever burn + `ft_burn`).
    pub total_snacc_burned: u128,
    /// Akumulasi subsidi treasury pada epoch berjalan (`I-04`).
    pub epoch_treasury_spent: u128,
    /// Snapshot saldo treasury di awal epoch untuk batas subsidi `500 bps` (`I-04`).
    pub epoch_treasury_initial: u128,
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
    /// `EN-105` — akumulasi volume feed (`effective_fee` `feed_strain`) epoch berjalan.
    pub epoch_feast_volume: u128,
    /// `EN-106` — zona makro terakhir yang terdeteksi (untuk event `zone_changed`).
    pub last_zone: MacroZone,
    /// `EN-104` — akumulasi diskon burst yang sudah diberikan epoch berjalan
    /// (cap bersama kuota `I-04` = 5% Pantry/epoch; anti-boros treasury).
    pub epoch_burst_discount_granted: u128,
    pub params: TermostatParams,
}

impl Default for MacroState {
    fn default() -> Self {
        Self {
            current_epoch: 0,
            last_update_ms: 0,
            volume_epoch: 0,
            total_snacc_supply: 0,
            total_snacc_burned: 0,
            epoch_treasury_spent: 0,
            epoch_treasury_initial: 0,
            nutrient_burst_bps: 0,
            nutrient_burst_until: 0,
            industrial_dividend_pool: 0,
            epoch_dividend_inflow: 0,
            epoch_aether_burned_total: 0,
            last_closed_epoch: 0,
            epoch_feast_volume: 0,
            last_zone: MacroZone::Normal,
            epoch_burst_discount_granted: 0,
            params: TermostatParams::default(),
        }
    }
}

/// Tampilan baca-saja `get_macro_state` (`F4`, `VC-005`) — mirror `MacroState` + `crucible_temp`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct MacroStateView {
    pub current_epoch: Epoch,
    pub last_update_ms: u64,
    pub volume_epoch: u128,
    pub total_snacc_supply: u128,
    pub total_snacc_burned: u128,
    pub epoch_treasury_spent: u128,
    pub epoch_treasury_initial: u128,
    pub nutrient_burst_bps: u16,
    pub nutrient_burst_until: Epoch,
    pub industrial_dividend_pool: u128,
    pub epoch_dividend_inflow: u128,
    pub epoch_aether_burned_total: u128,
    pub last_closed_epoch: Epoch,
    /// `EN-105` — akumulasi volume feed epoch berjalan (mirror `MacroState`).
    pub epoch_feast_volume: u128,
    /// `EN-106` — zona makro terakhir (mirror).
    pub last_zone: MacroZone,
    /// `EN-104` — akumulasi diskon burst epoch berjalan (mirror `MacroState`).
    pub epoch_burst_discount_granted: u128,
    pub params: TermostatParams,
    /// Suhu makro terhitung (`mK`, lazy decay tanpa cron — `ADR-04`).
    pub crucible_temp: u64,
}

impl MacroState {
    /// `V_decayed = max(0, volume_epoch - (now_sec - last_update_ms / 1000) * volume_decay_per_sec)` (`ADR-04`).
    pub fn decayed_volume(&self, now_ms: u64) -> u128 {
        let elapsed_sec = if self.last_update_ms == 0 {
            0
        } else {
            (now_ms / 1000).saturating_sub(self.last_update_ms / 1000)
        };
        let decay =
            (elapsed_sec as u128).saturating_mul(self.params.volume_decay_per_sec as u128);
        self.volume_epoch.saturating_sub(decay)
    }

    /// `CrucibleTemp = floor(1000 * V_decayed / max(total_snacc_supply, 1))` (`F4`, `ADR-04`).
    pub fn crucible_temp_at(&self, now_ms: u64) -> u64 {
        let v_decayed = self.decayed_volume(now_ms);
        let denom = std::cmp::max(self.total_snacc_supply, 1);
        (v_decayed.saturating_mul(1000) / denom) as u64
    }

    /// Bangun `MacroStateView` dengan `crucible_temp` pada `now_ms`.
    pub fn to_view(&self, now_ms: u64) -> MacroStateView {
        MacroStateView {
            current_epoch: self.current_epoch,
            last_update_ms: self.last_update_ms,
            volume_epoch: self.volume_epoch,
            total_snacc_supply: self.total_snacc_supply,
            total_snacc_burned: self.total_snacc_burned,
            epoch_treasury_spent: self.epoch_treasury_spent,
            epoch_treasury_initial: self.epoch_treasury_initial,
            nutrient_burst_bps: self.nutrient_burst_bps,
            nutrient_burst_until: self.nutrient_burst_until,
            industrial_dividend_pool: self.industrial_dividend_pool,
            epoch_dividend_inflow: self.epoch_dividend_inflow,
            epoch_aether_burned_total: self.epoch_aether_burned_total,
            last_closed_epoch: self.last_closed_epoch,
            epoch_feast_volume: self.epoch_feast_volume,
            last_zone: self.last_zone,
            epoch_burst_discount_granted: self.epoch_burst_discount_granted,
            params: self.params.clone(),
            crucible_temp: self.crucible_temp_at(now_ms),
        }
    }

    /// Burst aktif hanya dalam `nutrient_burst_until` epoch (kedaluwarsa otomatis
    /// saat berganti epoch, tanpa keeper — `ADR-27`, spec EN-01 §3.2).
    pub fn active_burst_bps(&self, epoch: Epoch) -> u16 {
        if epoch < self.nutrient_burst_until {
            self.nutrient_burst_bps
        } else {
            0
        }
    }
}

/// `EN-107` — entri leaderboard feeder (Great Feast; maksimal 3 per epoch).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct FeederEntry {
    pub account_id: AccountId,
    pub amount: u128,
}

/// Metadata `NEP-177` (subset yang dibutuhkan wallet: title/description/media).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct NftMetadataView {
    pub title: String,
    pub description: String,
    /// `data:image/svg+xml;base64,...` (`F7`, `ADR-03`).
    pub media: String,
}

/// View `nft_token` (`NEP-171`/`NEP-177`, `F7` — `VC-009`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct NftTokenView {
    pub token_id: String,
    pub owner_id: AccountId,
    pub metadata: NftMetadataView,
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
            owner_id: "alkemis.testnet".parse().unwrap(),
            vault_subaccount: "s-1.snacc-v001.near".parse().unwrap(),
            vitality: 2500,
            efficiency_bps: 10000,
            bmr_per_hour: 100,
            last_fed_sec: 3600,
            role_mask: 0b001,
            state_nonce: 7,
            pending_callbacks: 0,
            atp_balance: 42,
            pending_commitment: None,
            phenotype_trait: None,
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

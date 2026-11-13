//! State `D1–D8` — `VC-002` ([[Snacc - Model Data]]).
//!
//! Field/method names are copied verbatim from the spec: [[Snacc - Spec Antarmuka Kontrak]],
//! `ADR-22/25/26/29`, and the **Kamus Angka Resmi** ([[Snacc - Model Data]]).
//! All core structs use `#[near(serializers = [borsh, json])]` (DoD `VC-002`).

use near_sdk::{AccountId, near};

pub type StrainId = u64;
pub type TokenId = String; // `NEP-245`, e.g. `mt:spore-ignis`
pub type Epoch = u64;

/// Canonical constants (the Kamus Angka Resmi — do not use any other numbers).
pub mod constants {
    /// Guardrail mutasi TEE (`ADR-21`): `maxShiftBps = ±1500`.
    pub const MAX_SHIFT_BPS: u16 = 1500;
    /// Calcination Fever when `CrucibleTemp > 610` mK — **ECON-111 FINAL (owner
    /// decision 2026-11-08, supersedes 1400)**: Option B calibration `VOLUME_SCALE = 5`.
    /// (Hypothermia stays 600 mK; the Normal zone = [600, 610].)
    pub const FEVER_THRESHOLD_MK: u64 = 610;
    /// Hypothermia Rebate when `CrucibleTemp < 600` mK.
    pub const HYPOTHERMIA_THRESHOLD_MK: u64 = 600;
    /// `ft_burn 350 bps` (3.5%) on the catalyst cost during Fever.
    pub const FEVER_BURN_BPS: u16 = 350;
    /// A `200 bps` (2.0%) subsidy from the treasury during Hypothermia.
    pub const REBATE_BPS: u16 = 200;
    /// Cap drain treasury `500 bps`/epoch (5% Pantry/epoch, invariant `I-04`).
    pub const MAX_EPOCH_TREASURY_DRAIN_BPS: u16 = 500;
    /// Peluruhan volume kontinu (`VOLUME_DECAY_PER_SEC = 50`).
    pub const VOLUME_DECAY_PER_SEC: u64 = 50;
    /// Cell Lysis bounty: `10%` liquidator / `90%` `Rule-Bound Treasury`.
    pub const LIQUIDATOR_BOUNTY_BPS: u16 = 1000;
    /// A `2.5%` merger tax to the Treasury (`ADR-25`).
    pub const MERGER_TAX_BPS: u16 = 250;
    /// Hostile takeover premium: `book value + 15%` (`ADR-26` Tier 2).
    pub const HOSTILE_PREMIUM_BPS: u16 = 1500;
    /// Colony fence (`EN-08`).
    pub const MAX_COLONY_SIZE: u8 = 5;
    /// Population fence (`EN-08`).
    pub const MAX_GENERATION: u8 = 8;
    /// LC-06 — fusion (`splice_strains`): `BMR −15%` (Lifecycle LC-06, Narrative "Merge").
    pub const SPLICE_BMR_REDUCTION_BPS: u64 = 1500;
    /// The `advance_epoch` time gate: the duration of one epoch (seconds). The Kamus Angka
    /// has not nominalized `EPOCH_LEN` — the 2026-11-08 session decision: 24 hours (a daily loop);
    /// the demo advances time via `advance_sandbox_clock` (`ADR-22`).
    pub const EPOCH_DURATION_SEC: u64 = 86_400;
    pub const ERR_EPOCH_NOT_ENDED: &str = "ERR_EPOCH_NOT_ENDED";
    /// `ADR-22` — `advance_sandbox_clock` outside sandbox mode (an account that is not `.testnet`).
    pub const ERR_SANDBOX_CLOCK_DISABLED: &str = "ERR_SANDBOX_CLOCK_DISABLED";
    /// The cap of one `advance_sandbox_clock` call (30 days) — anti-overflow/typo.
    pub const MAX_SANDBOX_ADVANCE_HOURS: u64 = 720;
    /// The Nutrient Burst discount cap (`EN-01`, spec `0..2000`).
    pub const MAX_BURST_BPS: u16 = 2000;
    /// `F1` — vitalitas awal Strain Genesis = `8500` (`85.00%`).
    pub const INITIAL_VITALITY: u64 = 8500;
    /// `F1` — efisiensi awal = `10000` bps (`1.00x`).
    pub const INITIAL_EFFICIENCY_BPS: u16 = 10000;
    /// `F1` — initial `bmr_per_hour` per caste (`Katalog Molekul`).
    pub const BMR_EXTRACTOR_PER_HOUR: u64 = 100;
    pub const BMR_FERMENTER_PER_HOUR: u64 = 120;
    pub const BMR_SYNTHESIZER_PER_HOUR: u64 = 150;
    /// `ADR-25` / `I-08` — the Coasean role bitmask (must be single at Genesis).
    pub const ROLE_EXTRACTOR: u8 = 0b001;
    pub const ROLE_FERMENTER: u8 = 0b010;
    pub const ROLE_SYNTHESIZER: u8 = 0b100;
    /// `F1` — `20%` of the incubation fee to the `Rule-Bound Treasury`; `80%` Strain working capital.
    pub const STAKE_TREASURY_BPS: u128 = 2000;
    pub const BPS_DENOMINATOR: u128 = 10000;
    /// `F1` — deposit minimum `materialize_subaccount` = `0.02 NEAR` (20 milliNEAR).
    pub const MIN_MATERIALIZE_DEPOSIT_MILLINEAR: u128 = 20;
    /// `F1` — the number of genesis spores minted per incubation (`Spec F1`, not yet nominalized).
    pub const GENESIS_SPORE_AMOUNT: u64 = 1;
    /// `F1` — format sub-account kanonik `s-<id>.snacc-v001.near`.
    pub const VAULT_SUBACCOUNT_PREFIX: &str = "s-";
    /// `F1` — error code: the strain was already incubated.
    pub const ERR_STRAIN_ALREADY_INCUBATED: &str = "ERR_STRAIN_ALREADY_INCUBATED";
    /// `F1` — error code: the role bitmask is not a single bit.
    pub const ERR_ROLE_MASK_NOT_SINGLE: &str = "ERR_ROLE_MASK_NOT_SINGLE";
    /// `F1` — error code: the `materialize_subaccount` deposit is below 0.02 NEAR.
    pub const ERR_INSUFFICIENT_DEPOSIT: &str = "ERR_INSUFFICIENT_DEPOSIT";
    /// `F1` — error code: the caller is not the strain owner.
    pub const ERR_NOT_OWNED_BY_CALLEE: &str = "ERR_NOT_OWNED_BY_CALLEE";
    /// `F1` — error code: the `initial_snacc_stake` amount != the attached deposit.
    pub const ERR_STAKE_DEPOSIT_MISMATCH: &str = "ERR_STAKE_DEPOSIT_MISMATCH";
    /// `F1` — error code: strain not found.
    pub const ERR_STRAIN_NOT_FOUND: &str = "ERR_STRAIN_NOT_FOUND";
    /// `F1` — the `NEP-245` genesis spore token (`Katalog Molekul`, `DFD P1.3`).
    pub const GENESIS_SPORE_TOKEN_ID: &str = "mt:spore-ignis";
    /// `ADR-02` — the sub-account format. **Superseded by pipeline #7**: the suffix is now
    /// derived from `env::current_account_id()` (the contract parent always exists —
    /// hardcoding `.snacc-v001.near` fails on testnet). The constant is kept
    /// only as documentation of the mainnet format.
    pub const VAULT_SUBACCOUNT_SUFFIX: &str = ".snacc-v001.near";
    /// `F2` — metabolic recovery per successful transmutation: `vitality +500`.
    pub const METABOLIC_RECOVERY_VITALITY: u64 = 500;
    /// `I-02` — the vitality cap (`0..=10000`).
    pub const VITALITY_CAP: u64 = 10000;
    /// `F2` — `NEP-245` Stage 1 token ids (spores).
    pub const SPORE_TOKEN_IDS: [&str; 3] = ["mt:spore-ignis", "mt:spore-cryo", "mt:spore-lumin"];
    /// `F2` — Stage 2 token ids (enzymes, the Katalog Molekul recipes).
    pub const ENZYME_AMALGAM: &str = "mt:enzyme-amalgam";
    pub const ENZYME_REDACTASE: &str = "mt:enzyme-redactase";
    pub const ENZYME_SYNTHASE: &str = "mt:enzyme-synthase";
    /// `F2` — Stage 3 token ids (elixirs).
    pub const ELIXIR_HOMUNCULUS: &str = "mt:elixir-homunculus";
    pub const ELIXIR_HOMEOSTASIS: &str = "mt:elixir-homeostasis";
    pub const ELIXIR_AETHER: &str = "mt:elixir-aether";
    /// `F2` — supply-chain error codes.
    pub const ERR_STRAIN_STARVING: &str = "ERR_STRAIN_STARVING";
    pub const ERR_UNKNOWN_TOKEN: &str = "ERR_UNKNOWN_TOKEN";
    pub const ERR_ROLE_NOT_ALLOWED: &str = "ERR_ROLE_NOT_ALLOWED";
    pub const ERR_INSUFFICIENT_BALANCE: &str = "ERR_INSUFFICIENT_BALANCE";
    pub const ERR_RECIPE_NOT_MET: &str = "ERR_RECIPE_NOT_MET";
    pub const ERR_REQUIRES_TEE_ATTESTATION: &str = "ERR_REQUIRES_TEE_ATTESTATION";
    /// `F3` — the TEE verification model (Kamus Angka Resmi; the signed message: `<model>:<req_sha256>:<res_sha256>`, `S2.6`).
    pub const TEE_MODEL_ID: &str = "z-ai/glm-5.3-flash";
    /// `I-03` — guardrail mutasi TEE.
    pub const TEE_SHIFT_LIMIT_BPS: i64 = 1500;
    pub const TEE_VITALITY_BONUS_CAP: u64 = 500;
    /// `F3` — TEE error codes (the mismatch names come from the red-team spike `S1.9`).
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
    /// SEC-113 — overwriting a registered TEE key with a different attestation is rejected.
    pub const ERR_TEE_SIGNER_OVERWRITE: &str = "ERR_TEE_SIGNER_OVERWRITE";
    /// DK-070 — the TEE key to remove is not registered.
    pub const ERR_TEE_SIGNER_NOT_FOUND: &str = "ERR_TEE_SIGNER_NOT_FOUND";
    /// DK-073 — the TEE response exceeds the size cap.
    pub const ERR_TEE_RESPONSE_TOO_LARGE: &str = "ERR_TEE_RESPONSE_TOO_LARGE";
    /// DK-074 — `phenotype_trait` outside the whitelist / too long.
    pub const ERR_TEE_TRAIT_INVALID: &str = "ERR_TEE_TRAIT_INVALID";
    /// `F5` — merger error codes (the `ERR_NOT_SAME_OWNER` name from spec F5).
    pub const ERR_NOT_SAME_OWNER: &str = "ERR_NOT_SAME_OWNER";
    pub const ERR_SAME_STRAIN: &str = "ERR_SAME_STRAIN";
    pub const ERR_STRAIN_ALREADY_SPLICED: &str = "ERR_STRAIN_ALREADY_SPLICED";
    /// `F2`/`F5` — the nine `NEP-245` molecule ids (Katalog Molekul).
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
    /// `F6` — Cell Lysis error codes (names from spec F6 & the POV Flows).
    pub const ERR_STRAIN_NOT_INSOLVENT: &str = "ERR_STRAIN_NOT_INSOLVENT";
    pub const ERR_ALREADY_LIQUIDATED: &str = "ERR_ALREADY_LIQUIDATED";
    /// `N141` — error code: the `ft_transfer` receiver is not storage-registered yet.
    pub const ERR_ACCOUNT_NOT_REGISTERED: &str = "ERR_ACCOUNT_NOT_REGISTERED";
    /// `N141` — error code: `ft_transfer`/`ft_transfer_call` without exactly 1 yoctoNEAR.
    pub const ERR_YOCTO_REQUIRED: &str = "ERR_YOCTO_REQUIRED";
    /// `EN-126` — error code: `nft_transfer_call` with less prepaid gas than it must
    /// reserve for itself + the `nft_resolve_transfer` callback.
    pub const ERR_MORE_GAS_REQUIRED: &str = "ERR_MORE_GAS_REQUIRED";
    /// `DK-033/044` — error codes (exact names from the Spec Dividend Pool §6).
    pub const ERR_NOT_STRAIN_OWNER: &str = "ERR_NOT_STRAIN_OWNER";
    pub const ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE: &str =
        "ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE";
    pub const ERR_INSUFFICIENT_AETHER: &str = "ERR_INSUFFICIENT_AETHER";
    pub const ERR_EPOCH_NOT_CLOSED: &str = "ERR_EPOCH_NOT_CLOSED";
    pub const ERR_DIVIDEND_ALREADY_CLAIMED: &str = "ERR_DIVIDEND_ALREADY_CLAIMED";
    pub const ERR_DIVIDEND_CLAIM_EXPIRED: &str = "ERR_DIVIDEND_CLAIM_EXPIRED";
    pub const ERR_DIVIDEND_POOL_EMPTY: &str = "ERR_DIVIDEND_POOL_EMPTY";
    /// `DK-033/044` — documented constants (Spec Dividend Pool §3).
    /// `DIVIDEND_INFLOW_BPS = 5000` (50% of the non-burned fee, `ADR-28`).
    pub const DIVIDEND_INFLOW_BPS: u128 = 5000;
    /// `DIVIDEND_RATE_SCALE = 10^18` — avoids rounding that hurts small players.
    pub const DIVIDEND_RATE_SCALE: u128 = 1_000_000_000_000_000_000;
    /// `MAX_DIVIDEND_CLAIM_EPOCHS = 8` — the lookback claim limit, aligned with `MAX_GENERATION`.
    pub const MAX_DIVIDEND_CLAIM_EPOCHS: u64 = 8;
    /// `MIN_AETHER_BURN = 1` — prevents 0-amount spam (recorded decision: not nominalized in the spec).
    pub const MIN_AETHER_BURN: u128 = 1;
    /// `EN-01` — `BASE_BURST_BPS = 200` (2%), aligned with `REBATE_BPS` (Spec Enhancement §3.3).
    pub const BASE_BURST_BPS: u16 = 200;
    /// `EN-01` — `BURST_DURATION_EPOCHS = 1`.
    pub const BURST_DURATION_EPOCHS: u64 = 1;
    /// `N141` — the MVP minimum storage deposit = 5 milliNEAR (recorded decision).
    pub const STORAGE_DEPOSIT_MILLINEAR: u128 = 5;
    /// `F5` Path 2 — error code: the target is not in the Tier 2 Chapter 11 Distressed
    /// (`1 <= effective_vitality <= 2000`) window. A recorded-decision name (the spec does not name it).
    pub const ERR_NOT_DISTRESSED: &str = "ERR_NOT_DISTRESSED";
    /// `F5` Path 2 — the buyout payment = `book_value × (10000 + HOSTILE_PREMIUM_BPS) / 10000`
    /// (`ADR-26`: Book Value + 15% Restructuring Premium).
    pub const HOSTILE_SELLER_BPS: u128 = 9500; // 95% of the payment to the old owner
    pub const HOSTILE_TREASURY_BPS: u128 = 500; // 5% to the Rule-Bound Treasury
    /// `F5` Path 2 — the molecule `floor_price` is not nominalized in the spec → recorded
    /// decision: 1 `$SNACC` unit per molecule unit (transparent, easy to change).
    pub const MOLECULE_FLOOR_PRICE: u128 = 1;
    /// `EN-02`/`EN-105` — Great Feast (recorded decision: the target & band are not
    /// nominalized in the spec): the target feed volume/epoch = 5% of the total supply,
    /// the Goldilocks band = ±20% of the target.
    pub const FEAST_TARGET_BPS: u128 = 500;
    pub const FEAST_BAND_BPS: u128 = 2000;
    /// `EN-108` — dormancy: the feed upkeep rate drops to `UPKEEP_MIN_BPS` (`50` bps,
    /// from the D7 Connection Map) when `decayed_volume < threshold`. The threshold = recorded
    /// decision: 1% of the total supply (`DORMANCY_THRESHOLD_BPS`).
    pub const UPKEEP_MIN_BPS: u128 = 50;
    pub const DORMANCY_THRESHOLD_BPS: u64 = 100;
    /// `EN-109` — rescue feeding (recorded decision: linear; a minimum fee of `20%`
    /// when `effective_vitality -> 0`, a full `100%` at `effective_vitality = 10000`).
    pub const RESCUE_MIN_FEE_BPS: u128 = 2000;
    /// `EN-110` — the per-caste per-zone diet matrix (recorded decision; direction from
    /// `LC-14`: Fever → a cheaper Extractor, Famine → a cheaper Synthesizer; otherwise 0).
    pub const DIET_FEVER_EXTRACTOR_BPS: u16 = 2000;
    pub const DIET_FAMINE_SYNTHESIZER_BPS: u16 = 2000;
    /// `EN-122`/`EN-124` — colony-fence error codes (recorded-decision names).
    pub const ERR_COLONY_FULL: &str = "ERR_COLONY_FULL";
    pub const ERR_MAX_GENERATION: &str = "ERR_MAX_GENERATION";
    /// `ADR-30` — NPC error codes (recorded decision; the spec does not name them).
    pub const ERR_NPC_UNKNOWN: &str = "ERR_NPC_UNKNOWN";
    pub const ERR_NPC_OUT_OF_STOCK: &str = "ERR_NPC_OUT_OF_STOCK";
    pub const ERR_BAD_AMOUNT: &str = "ERR_BAD_AMOUNT";
    /// `ADR-30`/`ECON-111` — the base price `P_base = 100` (§13a; final ECON-111:
    /// `VOLUME_SCALE` is the only calibration knob).
    pub const P_BASE: u128 = 100;
    /// NPC alias (the old name kept for readability against the `ADR-30` spec).
    pub const NPC_P_BASE: u128 = P_BASE;
    /// `ECON-111` FINAL — `VOLUME_SCALE = 5`: the $SNACC-equivalent value multiplier per molecule
    /// unit on the K2/K3 legs of `V_epoch` (econ-report-3 §1; supersedes the EN-117 "1000 temporary").
    pub const VOLUME_SCALE: u128 = 5;
    /// EN-119 (owner decision 2026-11-08) — Nucleus: production output
    /// +`NUCLEUS_OUTPUT_BPS_PER_LEVEL` bps (10%)/level — multiplies the `minted` output
    /// of `transmute_spores_to_enzyme` & `synthesize_elixir`.
    pub const NUCLEUS_OUTPUT_BPS_PER_LEVEL: u128 = 1000;
    /// EN-119 — Lysosome: BMR −`LYSOSOME_BMR_BPS_PER_LEVEL` bps (10%)/level
    /// (longer lifespan); the effective BMR = `bmr × (10000 − 1000×lisosom)/10000` (min 0).
    pub const LYSOSOME_BMR_BPS_PER_LEVEL: u128 = 1000;
    /// EN-117 (owner decision 2026-11-08 — temperature is literal ATP): `V_epoch` =
    /// the economic legs + `atp_minted × ATP_VOLUME_SCALE`. The value is calibrated by the
    /// ECON-111 ATP-variant sweep (econ-report-4): `S_sim = 3000` → the ATP share is 37% of
    /// the feed (share = S/(5100+S)); contract: share = S_c/(10^15+S_c) → S_c = 6×10^14
    /// (a feed of 10^15 → 1 ATP → 6×10^14 volume). Fever 8.5%, population 73, solvent ✓.
    pub const ATP_VOLUME_SCALE: u128 = 600_000_000_000_000;
    /// `EN-114`/§13a — the initial ATP per new Strain (birth energy).
    pub const INITIAL_ATP: u128 = 50;
    /// `EN-114`/§13a — `0.001 NEAR catalyst = 1 ATP` (a yocto-scale fee unit).
    pub const ATP_YOCTO_UNIT: u128 = 1_000_000_000_000_000;
    /// `EN-118`/§13a — the maximum organel level.
    pub const MAX_ORGANEL_LEVEL: u8 = 5;
    /// `EN-114..118` — error codes (§13a/brief names).
    pub const ERR_NOT_ENOUGH_ATP: &str = "ERR_NOT_ENOUGH_ATP";
    /// Pipeline #5 — new error codes (recorded decision; the spec does not name them).
    pub const ERR_TRAIT_MISMATCH: &str = "ERR_TRAIT_MISMATCH";
    pub const ERR_BIRTHS_EXCEEDED: &str = "ERR_BIRTHS_EXCEEDED";
    pub const ERR_ALREADY_ALLOCATED: &str = "ERR_ALREADY_ALLOCATED";
    pub const ERR_ORGANEL_LEVEL_MAX: &str = "ERR_ORGANEL_LEVEL_MAX";
    /// `ADR-30` — NPC target stock (recorded decision; the Model Data tooltip example uses
    /// 500/500): extractor baseline 500, central-arb a large inventory of 1000, vulture 300.
    pub const NPC_TARGET_EXTRACTOR: u64 = 500;
    pub const NPC_TARGET_CENTRAL_ARB: u64 = 1000;
    pub const NPC_TARGET_VULTURE: u64 = 300;
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

/// Solvency status — a 3-rung ladder (`ADR-26`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum StrainStatus {
    /// Tier 1: `effective_vitality > 2000` (>20%) — fully protected.
    Active,
    /// Tier 2: `1..=2000` — the Chapter 11 window (`hostile_takeover_splice` opens).
    Distressed,
    /// Tier 3: `0` — Cell Lysis (`harvest_starving_strain` opens).
    Liquidated,
    /// `F5`: fully absorbed by a merger (`splice_strains`) — cannot be used again.
    SplicedBurned,
}

/// `SolvencyTier` (Model Data, Spec Antarmuka §2 `get_solvency_tier`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SolvencyTier {
    /// `> 2000` (>20%).
    Tier1Healthy,
    /// `1..=2000` (1..20%) — open to hostile takeover.
    Tier2Chapter11Distressed,
    /// `0` — open to `harvest_starving_strain`.
    Tier3Chapter7Insolvent,
}

/// The `Homunculus Strain` industrial role castes (`NEP-171`, `I-08`).
/// The enum guarantees a single role at Genesis; multi-bit only via `splice_strains`.
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

    /// Initial `bmr_per_hour` per caste (`F1`, `Katalog Molekul`): `100` / `120` / `150`.
    pub const fn bmr_per_hour(self) -> u64 {
        match self {
            StrainRole::Extractor => constants::BMR_EXTRACTOR_PER_HOUR,
            StrainRole::Fermenter => constants::BMR_FERMENTER_PER_HOUR,
            StrainRole::Synthesizer => constants::BMR_SYNTHESIZER_PER_HOUR,
        }
    }
}

/// Organel slots (`EN-07`): an MVP of 3 slots — `$ATP` allocation 1× per epoch.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OrganelleSlots {
    /// Mitochondria: raises `atp_minted` (EN-06).
    pub mitokondria: u8,
    /// Nucleus: raises BMR resilience.
    pub nukleus: u8,
    /// Lysosome: enlarges the Nutrient Burst on `snack`.
    pub lisosom: u8,
}

/// Alchemist profile (`D1`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct AlkemisProfile {
    pub account_id: AccountId,
    /// The second of first onboarding (`incubate_strain`).
    pub joined_sec: u64,
    /// The number of Strains owned (the `get_colony` cache, `EN-08`).
    pub strain_count: u8,
}

/// `StrainVessel` (`D2`) — the sovereign balance sheet of one `Homunculus Strain`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct StrainVessel {
    pub strain_id: StrainId,
    pub owner_id: AccountId,
    /// A keyless sub-account (`"keys": []`) holding the assets — `ADR-02`, `ADR-29`.
    pub vault_subaccount: AccountId,
    /// Vitality (the canonical ladder: `0`, `1..=2000`, `>2000`).
    pub vitality: u64,
    /// The `NEP-245` conversion efficiency (`F2`): `10000 bps = 1.00x`.
    pub efficiency_bps: u16,
    /// `BMR` — vitality decay per hour (`ADR-22`).
    pub bmr_per_hour: u64,
    /// Timestamp (seconds) of the last feeding.
    pub last_fed_sec: u64,
    /// Bitmask Coasean (`ADR-25`): `ROLE_EXTRACTOR=0b001`, `ROLE_FERMENTER=0b010`, `ROLE_SYNTHESIZER=0b100`.
    pub role_mask: u8,
    /// Monotonic nonce anti-drain (`ADR-29`).
    pub state_nonce: u64,
    /// Must be `0` during a mutation; locked while a cross-contract call runs (`ADR-29`).
    pub pending_callbacks: u8,
    /// Internal energy (`EN-06`) — **non-transferable, not a token**, without a `total_supply`.
    pub atp_balance: u128,
    /// `F3` (`ADR-21`): komitmen `req_sha256_hex` Phase 1 (`commit_genome_experiment`).
    pub pending_commitment: Option<String>,
    /// `F3`: the phenotype trait from a TEE mutation (the `phenotype_trait` payload).
    pub phenotype_trait: Option<String>,
    /// The generation produced by `splice_strains`/`divide_strain` (`EN-08`, max `MAX_GENERATION`).
    pub generation: u8,
    /// The organel set (`EN-07`); inherited on merger = endosymbiosis (`EN-120`).
    pub organel: OrganelleSlots,
    /// `EN-118`/`EN-120` — the last `allocate_atp` epoch (1× per epoch); reset to 0
    /// after a merger so ATP reallocation can happen in the same epoch (`EN-120`).
    pub last_allocate_epoch: Epoch,
    pub status: StrainStatus,
}

/// The read-only `get_strain` view (`F1`) — a `StrainVessel` mirror + `effective_vitality(now)`.
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
    pub last_allocate_epoch: Epoch,
    pub status: StrainStatus,
    /// The effective vitality computed at `now` (Lazy BMR, `ADR-22`).
    pub effective_vitality: u64,
}

impl StrainVessel {
    /// Build the view for `now_sec` (seconds).
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
            last_allocate_epoch: self.last_allocate_epoch,
            status: self.view_status(now_sec),
            effective_vitality: self.effective_vitality(now_sec),
        }
    }

    /// Audit W10 — the status for views: a stored `Active` status downgrades to
    /// `Distressed` when `1 <= effective_vitality <= 2000` (Tier 2, Lazy BMR —
    /// the status is computed in the view, keeperless). Tier 3 (`0`) stays `Active` until
    /// `harvest_starving_strain` (→ `Liquidated`); use `get_solvency_tier`.
    pub fn view_status(&self, now_sec: u64) -> StrainStatus {
        match self.status {
            StrainStatus::Active if (1..=2000).contains(&self.effective_vitality(now_sec)) => {
                StrainStatus::Distressed
            }
            ref s => s.clone(),
        }
    }

    /// `get_solvency_tier` (Spec Antarmuka §2) — tangga 3 tingkat `I-02`.
    pub fn solvency_tier(&self, now_sec: u64) -> SolvencyTier {
        match self.effective_vitality(now_sec) {
            0 => SolvencyTier::Tier3Chapter7Insolvent,
            1..=2000 => SolvencyTier::Tier2Chapter11Distressed,
            _ => SolvencyTier::Tier1Healthy,
        }
    }

    /// `effective_vitality = vitality - elapsed_hours * bmr_per_hour` (Lazy BMR).
    pub fn effective_vitality(&self, now_sec: u64) -> u64 {
        // EN-119 — Lisosom: BMR efektif = bmr × (10000 − 1000×level)/10000
        // (level ≤ 5 → BMR efektif ≥ 50% bmr; min 0 dijaga cap level).
        let bmr_eff = self.bmr_per_hour
            * (10_000u64
                .saturating_sub(
                    u64::from(self.organel.lisosom)
                        * u64::from(crate::state::constants::LYSOSOME_BMR_BPS_PER_LEVEL as u16),
                ))
            / 10_000;
        let elapsed_hours = now_sec.saturating_sub(self.last_fed_sec) / 3600;
        self.vitality
            .saturating_sub(elapsed_hours.saturating_mul(bmr_eff))
    }

    /// The balance/inventory mutation lock (`ADR-29`) — rejects when an async call is running
    /// or the nonce changed (the `ERC-6551` seller-drain attack).
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

    /// Every `$SNACC` balance/`NEP-245` inventory mutation bumps the nonce (`ADR-29`).
    pub fn record_mutation(&mut self) {
        self.state_nonce = self.state_nonce.saturating_add(1);
    }
}

/// Macro zone (`EN-106`) — stored for `zone_changed` transition detection.
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

/// Monetary thermostat parameters (`D7`) — every value follows the Kamus Angka Resmi.
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
    /// `EN-108` — the dormancy threshold (`%` of supply × 100): dormant when
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

/// `MacroState` (`D7`) — the `CrucibleTemp` macro state + Nutrient Burst + dividends.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct MacroState {
    pub current_epoch: Epoch,
    /// Audit W6 — the running epoch may only be closed when `now_sec() >= epoch_ends_sec`.
    pub epoch_ends_sec: u64,
    /// Block timestamp (ms) of the last update — **from `env::block_timestamp_ms()`** (`V-02`).
    pub last_update_ms: u64,
    /// `V_epoch` — the decayed activity volume for `CrucibleTemp = 1000 × V / M_supply`.
    pub volume_epoch: u128,
    /// The circulating `$SNACC` total supply (`I-01`, grows on `incubate_strain`, shrinks on burn).
    pub total_snacc_supply: u128,
    /// The accumulated burned `$SNACC` (`I-01`, Fever burn + `ft_burn`).
    pub total_snacc_burned: u128,
    /// The accumulated treasury subsidy of the running epoch (`I-04`).
    pub epoch_treasury_spent: u128,
    /// A snapshot of the treasury balance at epoch start for the `500 bps` subsidy cap (`I-04`).
    pub epoch_treasury_initial: u128,
    /// Diskon feed aktif (`EN-01`, `0..=MAX_BURST_BPS`).
    pub nutrient_burst_bps: u16,
    /// The epoch where the burst ends (`EN-01`).
    pub nutrient_burst_until: Epoch,
    /// `industrial_dividend_pool` (`DK-033`) — akumulasi 50% fee non-burn.
    pub industrial_dividend_pool: u128,
    /// The running epoch's dividend inflow (`DK-033`).
    pub epoch_dividend_inflow: u128,
    /// The total `mt:elixir-aether` burned in the running epoch (`DK-044`).
    pub epoch_aether_burned_total: u128,
    /// The last closed epoch (`advance_epoch`, `DK-033`).
    pub last_closed_epoch: Epoch,
    /// `EN-105` — the running epoch's accumulated feed volume (`effective_fee` of `feed_strain`).
    pub epoch_feast_volume: u128,
    /// `EN-106` — the last detected macro zone (for the `zone_changed` event).
    pub last_zone: MacroZone,
    /// `EN-104` — the burst discount already granted in the running epoch
    /// (capped together with the `I-04` quota = 5% Pantry/epoch; protects the treasury from waste).
    pub epoch_burst_discount_granted: u128,
    /// Pipeline #6 — aggregate bookkeeping for the `V-04` invariant (`get_global_solvency`):
    /// LookupMap cannot be iterated, so the aggregates are maintained incrementally by
    /// the `set_strain_snacc`/`set_account_balance` helpers (the only ledger write paths).
    pub sum_strain_snacc: u128,
    pub sum_account_balances: u128,
    pub sum_npc_reserve: u128,
    /// Pipeline #6 — the accumulated catalyst fees (NEAR-yocto, external inflow) entering
    /// the treasury/pool buckets — the invariant's income side beyond minted `$SNACC`.
    pub total_catalyst_fees: u128,
    /// `EN-124` — demographics counters (D6): births = incubation + divide;
    /// mergers = splice + hostile; lyses = harvest. `total_divides` = the derived
    /// bookkeeping (the births share of divide) for the invariant gate.
    pub total_births: u128,
    pub total_mergers: u128,
    pub total_lyses: u128,
    pub total_divides: u128,
    pub params: TermostatParams,
}

impl Default for MacroState {
    fn default() -> Self {
        Self {
            current_epoch: 0,
            epoch_ends_sec: 0,
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
            sum_strain_snacc: 0,
            sum_account_balances: 0,
            sum_npc_reserve: 0,
            total_catalyst_fees: 0,
            total_births: 0,
            total_mergers: 0,
            total_lyses: 0,
            total_divides: 0,
            params: TermostatParams::default(),
        }
    }
}

/// The read-only `get_macro_state` view (`F4`, `VC-005`) — a `MacroState` mirror + `crucible_temp`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct MacroStateView {
    pub current_epoch: Epoch,
    /// Audit W6 — the running epoch may only be closed when `now_sec() >= epoch_ends_sec`.
    pub epoch_ends_sec: u64,
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
    /// `EN-105` — the running epoch's accumulated feed volume (mirror of `MacroState`).
    pub epoch_feast_volume: u128,
    /// `EN-106` — the last macro zone (mirror).
    pub last_zone: MacroZone,
    /// `EN-104` — the running epoch's burst discount accumulator (mirror of `MacroState`).
    pub epoch_burst_discount_granted: u128,
    /// Pipeline #6 — the aggregate bookkeeping (mirror) + the `V-04` drift.
    pub sum_strain_snacc: u128,
    pub sum_account_balances: u128,
    pub sum_npc_reserve: u128,
    pub total_catalyst_fees: u128,
    /// `EN-124` — demographics counters (mirror).
    pub total_births: u128,
    pub total_mergers: u128,
    pub total_lyses: u128,
    pub total_divides: u128,
    pub params: TermostatParams,
    /// The computed macro temperature (`mK`, cron-free lazy decay — `ADR-04`).
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
        // DK-026 — temperature = 1000 × (V/M): a VELOCITY ratio, unit-invariant — V and M
        // are both in yocto, so proportional activity (V = c×M) yields the same temperature
        // across supplies of 10^18..10^30 yocto (property test qa_dk026_*).
        // Saturating + a u64 clamp: no wrap at extreme scales.
        let t = v_decayed.saturating_mul(1000) / denom;
        t.min(u64::MAX as u128) as u64
    }

    /// Build the `MacroStateView` with `crucible_temp` at `now_ms`.
    pub fn to_view(&self, now_ms: u64) -> MacroStateView {
        MacroStateView {
            current_epoch: self.current_epoch,
            epoch_ends_sec: self.epoch_ends_sec,
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
            sum_strain_snacc: self.sum_strain_snacc,
            sum_account_balances: self.sum_account_balances,
            sum_npc_reserve: self.sum_npc_reserve,
            total_catalyst_fees: self.total_catalyst_fees,
            total_births: self.total_births,
            total_mergers: self.total_mergers,
            total_lyses: self.total_lyses,
            total_divides: self.total_divides,
            params: self.params.clone(),
            crucible_temp: self.crucible_temp_at(now_ms),
        }
    }

    /// The burst is active only before the `nutrient_burst_until` epoch (expires automatically
    /// when the epoch changes, keeperless — `ADR-27`, spec EN-01 §3.2).
    pub fn active_burst_bps(&self, epoch: Epoch) -> u16 {
        if epoch < self.nutrient_burst_until {
            self.nutrient_burst_bps
        } else {
            0
        }
    }
}

/// `EN-107` — a feeder leaderboard entry (Great Feast; at most 3 per epoch).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct FeederEntry {
    pub account_id: AccountId,
    pub amount: u128,
}

/// `NEP-177` metadata (the subset wallets need: title/description/media).
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

/// On-chain autonomous NPC merchant & arbitrageur agents (`D4` / `ADR-30`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct NpcMerchantAgent {
    pub npc_id: String,
    pub wallet_account: String,
    pub role_archetype: String,
    pub snacc_reserve: u128,
    pub spores_stock: u64,
    pub target_spores_stock: u64,
    pub base_spore_price: u128,
    pub trades_executed: u64,
}

/// The on-chain NPC quote output + reasoning tooltip (`ADR-30`, Model Data — json serialization).
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
pub struct NpcQuoteView {
    pub npc_id: String,
    pub unit_price_vita: u128,
    pub total_cost_snacc: u128,
    pub npc_spores_stock_after: u64,
    pub crucible_temp_mk: u64,
    pub reasoning_tooltip: String,
}

/// TEE attestation proof (`D5`) — stored from `apply_tee_mutation_zero_trust` (`ADR-21`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct TeeAttestation {
    pub chat_id: String,
    pub model: String,
    pub req_sha256_hex: String,
    pub res_sha256_hex: String,
    /// The enclave's ed25519 pubkey (hex), verified via `env::ed25519_verify`.
    pub signer_pubkey_hex: String,
}

/// M&A & liquidation log (`D8`).
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
            vault_subaccount: "s-1.snacc-v001.testnet".parse().unwrap(),
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
            last_allocate_epoch: 3,
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
    fn vc002_locked_reference_numbers() {
        let p = TermostatParams::default();
        // ECON-111 FINAL (owner decision 2026-11-08): Fever >610 mK (supersedes 1400).
        assert_eq!(p.fever_threshold_mk, 610);
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
        let v = sample_strain(); // last_fed 3600, bmr 100/h
        assert_eq!(v.effective_vitality(3600), 2500);
        assert_eq!(v.effective_vitality(3600 + 5 * 3600), 2000); // 5 hours × 100
        assert_eq!(v.effective_vitality(3600 + 100 * 3600), 0); // saturates to 0
    }

    #[test]
    fn vc002_record_mutation_bumps_nonce() {
        let mut v = sample_strain();
        v.record_mutation();
        assert_eq!(v.state_nonce, 8);
    }

    #[test]
    #[should_panic(expected = "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED")]
    fn vc002_nonce_mismatch_rejected() {
        let v = sample_strain();
        v.assert_atomic_ready(99);
    }

    #[test]
    #[should_panic(expected = "ERR_PENDING_ASYNC_CALLBACK_LOCKED")]
    fn vc002_pending_callback_locks() {
        let mut v = sample_strain();
        v.pending_callbacks = 1;
        v.assert_atomic_ready(7);
    }
}

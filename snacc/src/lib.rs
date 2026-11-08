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

use near_sdk::json_types::U128;
use near_sdk::serde::Deserialize;
use near_sdk::store::{LookupMap, LookupSet, Vector};
use near_sdk::{env, near, AccountId, NearToken, PanicOnDefault, Promise};
use state::{
    constants, AlkemisProfile, Epoch, FeederEntry, MacroState, MacroStateView, MacroZone,
    MnaLogEntry, NftMetadataView, NftTokenView, OrganelleSlots, StrainId, StrainRole,
    StrainStatus, StrainVessel, StrainVesselView, TeeAttestation, TokenId,
};

/// Payload mutasi genom — `choices[0].message.content` (JSON di dalam JSON, `F3`/`I-03`).
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct MutationPayload {
    strain_id: u64,
    efficiency_shift_bps: i64,
    vitality_bonus: u64,
    #[serde(default)]
    phenotype_trait: Option<String>,
}

/// Envelope respons chat (`/v1/chat/completions`) — ekstraksi `chat_id` anti-replay.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct ChatEnvelope {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct ChatMessage {
    content: String,
}

/// State kontrak — `D1–D8` (`D9` = event `NEP-297`, tanpa state). `VC-002`.
#[near(contract_state)]
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
    /// `D5`/`I-16` (`ADR-33`): registri kunci publik `ed25519` enclave TEE (`pubkey -> attested_at_sec`).
    pub attested_tee_signers: LookupMap<[u8; 32], u64>,
    /// Pemilik kontrak (gerbang `register_attested_signer`) — bootstrap saat registrasi pertama.
    pub owner_id: Option<AccountId>,
    /// `F6`: bounty likuidator per **akun pemanggil** (`D6` perluasan) —
    /// kini JUGA ledger `NEP-141` `$SNACC` per akun (`N141-101..106`):
    /// `ft_balance_of(account)` = `account_balances` saja (keputusan tercatat).
    pub account_balances: LookupMap<AccountId, u128>,
    /// `N141` — indeks strain per pemilik (untuk `get_total_wealth`; `LookupMap`
    /// tidak bisa diiterasi, jadi daftar id dipelihara saat `incubate_strain`).
    pub strains_by_owner: LookupMap<AccountId, Vector<StrainId>>,
    /// `N141` — registry storage MVP: akun -> total milliNEAR yang didepositkan.
    pub registered_storage: LookupMap<AccountId, u128>,
    /// `DK-033` — `epoch_dividend_rate[epoch]` (skala `DIVIDEND_RATE_SCALE`).
    pub epoch_dividend_rate: LookupMap<Epoch, u128>,
    /// `DK-033` — `epoch_aether_burned[(epoch, strain_id)] -> amount`.
    pub epoch_aether_burned: LookupMap<(Epoch, StrainId), u128>,
    /// `DK-033` — anti double-claim `(epoch, strain_id)`.
    pub dividend_claimed: LookupSet<(Epoch, StrainId)>,
    /// `EN-107` — kontribusi feed per `(epoch, akun)` (Great Feast).
    pub feast_contributions: LookupMap<(Epoch, AccountId), u128>,
    /// `EN-107` — leaderboard top-3 feeder per epoch (batas 3 entri, gas aman).
    pub epoch_top_feeders: LookupMap<Epoch, Vec<FeederEntry>>,
    /// `D6` Kas `$SNACC` per Strain (neraca `vault_subaccount`).
    pub snacc_balances: LookupMap<StrainId, u128>,
    /// `D6` `Rule-Bound Treasury` — tanpa fungsi penarikan admin (`I-05`).
    pub treasury: u128,
    /// `D7` Parameter & state termostat moneter (`CrucibleTemp`, burst, dividen).
    pub macro_state: MacroState,
    /// `D8` Log M&A & likuidasi.
    pub mna_logs: Vector<MnaLogEntry>,
    /// `F1` — counter `StrainId` berurutan mulai `1`.
    pub next_strain_id: StrainId,
}

#[near]
impl SnaccContract {
    #[init]
    pub fn new() -> Self {
        Self {
            alchemists: LookupMap::new(b"a".to_vec()),
            strains: LookupMap::new(b"s".to_vec()),
            inventories: LookupMap::new(b"i".to_vec()),
            tee_attestations: LookupMap::new(b"t".to_vec()),
            used_tee_chat_ids: LookupSet::new(b"u".to_vec()),
            attested_tee_signers: LookupMap::new(b"p".to_vec()),
            owner_id: None,
            account_balances: LookupMap::new(b"l".to_vec()),
            strains_by_owner: LookupMap::new(b"o".to_vec()),
            registered_storage: LookupMap::new(b"r".to_vec()),
            epoch_dividend_rate: LookupMap::new(b"e".to_vec()),
            epoch_aether_burned: LookupMap::new(b"f".to_vec()),
            dividend_claimed: LookupSet::new(b"g".to_vec()),
            feast_contributions: LookupMap::new(b"h".to_vec()),
            epoch_top_feeders: LookupMap::new(b"k".to_vec()),
            snacc_balances: LookupMap::new(b"b".to_vec()),
            treasury: 0,
            macro_state: MacroState::default(),
            mna_logs: Vector::new(b"m".to_vec()),
            next_strain_id: 1,
        }
    }

    /// Penanda versi protokol — dipakai smoke test & verifikasi deploy.
    pub fn protocol_version(&self) -> &'static str {
        "snacc-v001"
    }

    /// `F1` / `P1` — cetak `Homunculus Strain` Genesis (`NEP-171`).
    ///
    /// `initial_snacc_stake` adalah nominal split `80/20`; bila ada attached
    /// deposit NEAR, nominal dan deposit wajib cocok (bila argumen `0`, nominal
    /// diambil dari attached deposit).
    #[payable]
    pub fn incubate_strain(
        &mut self,
        role: StrainRole,
        initial_snacc_stake: U128,
        initial_spores: u64,
    ) -> StrainVesselView {
        // `I-08`: `StrainRole` menjamin satu bit peran; assert menegaskan bitmask.
        let role_mask = role.to_role_mask();
        assert!(
            matches!(
                role_mask,
                constants::ROLE_EXTRACTOR
                    | constants::ROLE_FERMENTER
                    | constants::ROLE_SYNTHESIZER
            ),
            "{}", constants::ERR_ROLE_MASK_NOT_SINGLE
        );

        let attached = env::attached_deposit().as_yoctonear();
        let stake: u128 = if initial_snacc_stake.0 == 0 {
            assert!(
                attached > 0,
                "{}", constants::ERR_INSUFFICIENT_DEPOSIT
            );
            attached
        } else {
            assert!(
                attached == 0 || attached == initial_snacc_stake.0,
                "{}", constants::ERR_STAKE_DEPOSIT_MISMATCH
            );
            initial_snacc_stake.0
        };

        // `F1`: `20%` fee → `Rule-Bound Treasury` (`D6`),
        // `80%` → modal kerja Strain (sisa pembulatan menjaga konservasi `I-01`).
        let to_treasury = stake * constants::STAKE_TREASURY_BPS / constants::BPS_DENOMINATOR;
        let to_strain = stake - to_treasury;

        let owner_id = env::predecessor_account_id();
        let now_sec = env::block_timestamp() / 1_000_000_000;
        let strain_id = self.next_strain_id;
        self.next_strain_id += 1;

        let vault_subaccount: AccountId =
            format!("s-{}{}", strain_id, constants::VAULT_SUBACCOUNT_SUFFIX)
                .parse()
                .unwrap();

        let vessel = StrainVessel {
            strain_id,
            owner_id: owner_id.clone(),
            vault_subaccount,
            vitality: constants::INITIAL_VITALITY,
            efficiency_bps: constants::INITIAL_EFFICIENCY_BPS,
            bmr_per_hour: role.bmr_per_hour(),
            last_fed_sec: now_sec,
            role_mask,
            state_nonce: 0,
            pending_callbacks: 0,
            atp_balance: 0,
            pending_commitment: None,
            phenotype_trait: None,
            generation: 0,
            organel: OrganelleSlots::default(),
            status: StrainStatus::Active,
        };

        // `D2` ledger, `D6` neraca internal & treasury, `D7` suplai `$SNACC` (`I-01`).
        let _ = self.strains.insert(strain_id, vessel.clone());
        let _ = self.snacc_balances.insert(strain_id, to_strain);
        self.treasury += to_treasury;
        self.macro_state.total_snacc_supply += stake;
        if self.macro_state.current_epoch == 0 {
            self.macro_state.epoch_treasury_initial += to_treasury;
        }

        // `D3` — `mt_mint` spora genesis `NEP-245` ke inventori `(strain_id, token_id)`.
        let spore_key: (StrainId, TokenId) =
            (strain_id, constants::GENESIS_SPORE_TOKEN_ID.to_string());
        let spores = self.inventories.get(&spore_key).copied().unwrap_or(0);
        let _ = self
            .inventories
            .insert(spore_key, spores + initial_spores as u128);

        // `D9` — emit NEP-297 `strain_created`.
        events::emit_event_json(&format!(
            r#"{{"kind":"strain_created","strain_id":{},"owner_id":"{}","role_mask":{},"initial_snacc_stake":{},"spores_minted":{}}}"#,
            strain_id, owner_id, role_mask, stake, initial_spores
        ));

        // `D1` — registry alkemis (`joined_sec` saat pertama, `strain_count += 1`).
        let mut profile = self
            .alchemists
            .get(&owner_id)
            .cloned()
            .unwrap_or(AlkemisProfile {
                account_id: owner_id.clone(),
                joined_sec: now_sec,
                strain_count: 0,
            });
        profile.strain_count = profile.strain_count.saturating_add(1);
        let _ = self.alchemists.insert(owner_id.clone(), profile);

        // `N141` — indeks strain per pemilik (untuk `get_total_wealth`);
        // prefix `Vector` wajib unik per akun (pola nested-collection near-sdk).
        match self.strains_by_owner.get_mut(&owner_id) {
            Some(owned) => owned.push(strain_id),
            None => {
                let mut owned = Vector::new(format!("o:{}", owner_id).into_bytes());
                owned.push(strain_id);
                let _ = self.strains_by_owner.insert(owner_id, owned);
            }
        }

        vessel.to_view(now_sec)
    }

    /// `F1` / `P1` (`ADR-02`, `ADR-24`) — buat akun NEAR fisik sub-account.
    #[payable]
    pub fn materialize_subaccount(&mut self, strain_id: StrainId) -> Promise {
        let vessel = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND);
        let owner_id = vessel.owner_id.clone();
        assert_eq!(
            env::predecessor_account_id(),
            owner_id,
            "{}", constants::ERR_NOT_OWNED_BY_CALLEE
        );

        let deposit = env::attached_deposit();
        assert!(
            deposit
                >= NearToken::from_millinear(constants::MIN_MATERIALIZE_DEPOSIT_MILLINEAR),
            "ERR_INSUFFICIENT_DEPOSIT"
        );

        let sub_id: AccountId =
            format!("s-{}{}", strain_id, constants::VAULT_SUBACCOUNT_SUFFIX)
                .parse()
                .unwrap();

        Promise::new(sub_id.clone())
            .create_account()
            .transfer(deposit)
            .then(
                Self::ext(env::current_account_id()).on_subaccount_created(
                    strain_id,
                    sub_id,
                    owner_id,
                    U128(deposit.as_yoctonear()),
                ),
            )
    }

    /// `ADR-24` — callback anti-spoofing (`#[private]`). Kegagalan
    /// `create_account` tidak merusak neraca internal; deposit NEAR direfund.
    #[private]
    pub fn on_subaccount_created(
        &mut self,
        strain_id: StrainId,
        sub_account_id: AccountId,
        owner_id: AccountId,
        deposit_yocto: U128,
    ) {
        if env::promise_result_checked(0, usize::MAX).is_err() {
            let _ = Promise::new(owner_id).transfer(NearToken::from_yoctonear(deposit_yocto.0));
            near_sdk::log!(
                "ERR_SUBACCOUNT_CREATION_FAILED_REFUNDED strain_id={} sub_account_id={}",
                strain_id,
                sub_account_id
            );
        } else {
            near_sdk::log!(
                "subaccount_materialized strain_id={} sub_account_id={}",
                strain_id,
                sub_account_id
            );
        }
    }

    /// `F1` — view baca-saja satu Strain dengan `effective_vitality` untuk `now`.
    pub fn get_strain(&self, strain_id: StrainId) -> StrainVesselView {
        let vessel = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND);
        vessel.to_view(env::block_timestamp() / 1_000_000_000)
    }

    // =====================================================================
    // VC-005 — `F4`: Termostat Makro `CrucibleTemp` (`ADR-04`, `I-01`, `I-04`)
    // =====================================================================

    /// `F4` / `ADR-04` — suhu makro `CrucibleTemp` (`mK`) dengan lazy decay tanpa cron:
    /// `floor(1000 * V_decayed / max(total_snacc_supply, 1))`.
    pub fn get_crucible_temp(&self) -> u64 {
        let now_ms = env::block_timestamp() / 1_000_000;
        self.macro_state.crucible_temp_at(now_ms)
    }

    /// `F4` — view `MacroState` + `crucible_temp` terhitung pada timestamp saat ini.
    pub fn get_macro_state(&self) -> MacroStateView {
        let now_ms = env::block_timestamp() / 1_000_000;
        self.macro_state.to_view(now_ms)
    }

    /// `F4` — transisi epoch (`VC-005` + `DK-033` + `EN-01`):
    /// 1. Tutup epoch dividen: `epoch_dividend_rate[epoch] = epoch_dividend_inflow × SCALE
    ///    / max(epoch_aether_burned_total, 1)` dihitung saat penutupan (anti-manipulasi urutan
    ///    tx), `industrial_dividend_pool += epoch_dividend_inflow`, reset counter,
    ///    `last_closed_epoch += 1`, emit `epoch_dividend_closed`.
    /// 2. Reset kuota treasury `I-04` (`epoch_treasury_spent = 0`, snapshot `initial = treasury`).
    /// 3. Reset akumulator diskon burst `EN-104`; burst kedaluwarsa otomatis via
    ///    `active_burst_bps` (tanpa keeper — `ADR-27`).
    pub fn advance_epoch(&mut self) {
        // --- `DK-033`: penutupan epoch dividen (V-02: waktu dari blok, bukan parameter). ---
        let closing = self.macro_state.current_epoch;
        let total_burned = self.macro_state.epoch_aether_burned_total;
        let rate = self.macro_state.epoch_dividend_inflow
            * constants::DIVIDEND_RATE_SCALE
            / std::cmp::max(total_burned, 1);
        let _ = self.epoch_dividend_rate.insert(closing, rate);
        self.macro_state.industrial_dividend_pool = self
            .macro_state
            .industrial_dividend_pool
            .saturating_add(self.macro_state.epoch_dividend_inflow);
        self.macro_state.epoch_dividend_inflow = 0;
        self.macro_state.epoch_aether_burned_total = 0;
        self.macro_state.last_closed_epoch = self.macro_state.last_closed_epoch.saturating_add(1);

        // --- `EN-105`: evaluasi The Great Feast (band Goldilocks) saat epoch ditutup. ---
        // Keputusan tercatat: target = 5% supply, band ±20% (FEAST_TARGET_BPS/FEAST_BAND_BPS);
        // MVP feast hanya menilai & memancarkan verdict — bonus token tidak ternominalisasi.
        let feast_volume = self.macro_state.epoch_feast_volume;
        let feast_target = self.macro_state.total_snacc_supply * constants::FEAST_TARGET_BPS
            / constants::BPS_DENOMINATOR;
        let feast_band =
            feast_target * constants::FEAST_BAND_BPS / constants::BPS_DENOMINATOR;
        let verdict = if feast_volume
            >= feast_target.saturating_sub(feast_band)
            && feast_volume <= feast_target + feast_band
        {
            "GOLDILOCKS"
        } else if feast_volume > feast_target + feast_band {
            "OVERFED"
        } else {
            "UNDERFED"
        };
        self.macro_state.epoch_feast_volume = 0;
        events::emit_event_json(&format!(
            r#"{{"kind":"feast_result","epoch":{},"volume":{},"target":{},"verdict":"{}"}}"#,
            closing, feast_volume, feast_target, verdict
        ));

        // --- `VC-005`: reset kuota `I-04`. ---
        self.macro_state.current_epoch = self.macro_state.current_epoch.saturating_add(1);
        self.macro_state.epoch_treasury_spent = 0;
        self.macro_state.epoch_treasury_initial = self.treasury;

        // --- `EN-104`: reset akumulator diskon burst. ---
        self.macro_state.epoch_burst_discount_granted = 0;

        events::emit_event_json(&format!(
            r#"{{"kind":"epoch_dividend_closed","epoch":{},"pool":{},"total_burned":{},"rate":{}}}"#,
            closing,
            self.macro_state.industrial_dividend_pool,
            total_burned,
            rate
        ));
    }

    /// `F4` / `I-01` — bakar `$SNACC` dari neraca `snacc_balances[strain_id]`.
    /// TODO(VC-006/NEP-141): selaraskan signature `ft_burn(amount: U128)` saat ledger NEP-141 global tersedia.
    pub fn ft_burn(&mut self, strain_id: StrainId, amount: U128) {
        // `SEC-101` (temuan `free-pi-cli` 2026-11-07): WAJIB cek pemanggil — hanya
        // pemilik Strain (*pemegang token*) yang boleh membakar saldonya sendiri.
        let owner = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .owner_id
            .clone();
        assert_eq!(
            env::predecessor_account_id(),
            owner,
            "{}",
            constants::ERR_NOT_OWNED_BY_CALLEE
        );
        let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        assert!(
            bal >= amount.0,
            "{}",
            constants::ERR_INSUFFICIENT_BALANCE
        );
        let _ = self.snacc_balances.insert(strain_id, bal - amount.0);
        self.macro_state.total_snacc_supply = self
            .macro_state
            .total_snacc_supply
            .saturating_sub(amount.0);
        self.macro_state.total_snacc_burned = self
            .macro_state
            .total_snacc_burned
            .saturating_add(amount.0);
        if let Some(mut v) = self.strains.get(&strain_id).cloned() {
            v.record_mutation();
            let _ = self.strains.insert(strain_id, v);
        }
        // `N141` — event `NEP-297` ber-standard `nep141` (`ft_burn`).
        events::emit_standard_event(
            "nep141",
            "ft_burn",
            &format!(
                r#"{{"owner_id":"{}","amount":{}}}"#,
                self.strains
                    .get(&strain_id)
                    .map(|v| v.owner_id.to_string())
                    .unwrap_or_default(),
                amount.0
            ),
        );
    }

    // =====================================================================
    // VC-004 — `F2`: rantai pasok 3-tahap `NEP-245` + Lazy BMR (`ADR-12/13`)
    // =====================================================================

    /// Gerbang `Lazy BMR` (`F2`): tolak bila `effective_vitality == 0` (`ERR_STRAIN_STARVING`).
    fn assert_not_starving(v: &StrainVessel, now_sec: u64) -> u64 {
        let eff = v.effective_vitality(now_sec);
        assert!(eff > 0, "{}", constants::ERR_STRAIN_STARVING);
        eff
    }

    /// Gerbang kasta: `role_mask` wajib memuat bit peran (`ERR_ROLE_NOT_ALLOWED`).
    fn assert_role(v: &StrainVessel, role_bit: u8) {
        assert!(
            v.role_mask & role_bit != 0,
            "{}",
            constants::ERR_ROLE_NOT_ALLOWED
        );
    }

    /// Katalis `$SNACC` (`F2`/`F4`, `VC-005`):
    /// - Validasi `attached_deposit == fee` (`ERR_STAKE_DEPOSIT_MISMATCH`).
    /// - Perbarui `volume_epoch` (termasuk lazy decay sejak `last_update_ms`) + `last_update_ms`.
    /// - Hitung `temp = get_crucible_temp()`:
    ///   * `temp > 1400` (Calcination Fever): `burn = fee * 350 / 10000`, `total_snacc_supply -= burn`,
    ///     `total_snacc_burned += burn` (`I-01`), sisa `fee - burn` masuk `treasury`.
    ///   * `temp < 600` (Hypothermia): `fee` masuk `treasury`; `rebate = fee * 200 / 10000`
    ///     dibatasi sisa kuota epoch `epoch_treasury_initial * 500 / 10000 - epoch_treasury_spent` (`I-04`),
    ///     dibayar dari `treasury` ke `snacc_balances[strain_id]`.
    ///   * `600..=1400` (Zona tengah): `fee` masuk `treasury` tanpa burn/rebate.
    fn take_catalyst_fee(&mut self, strain_id: StrainId, fee: &U128) {
        assert_eq!(
            env::attached_deposit().as_yoctonear(),
            fee.0,
            "{}",
            constants::ERR_STAKE_DEPOSIT_MISMATCH
        );
        self.apply_thermostat_fee(strain_id, fee.0);
    }

    /// Jalur termostat untuk fee internal (`feed_strain` `EN-103`): sama seperti
    /// `take_catalyst_fee` tanpa asersi attached deposit.
    fn apply_thermostat_fee(&mut self, strain_id: StrainId, fee: u128) {
        let now_ms = env::block_timestamp() / 1_000_000;
        let v_decayed = self.macro_state.decayed_volume(now_ms);
        self.macro_state.volume_epoch = v_decayed.saturating_add(fee);
        self.macro_state.last_update_ms = now_ms;

        let temp = self.get_crucible_temp();
        // `EN-106` — deteksi transisi zona makro → event `zone_changed` (tanpa keeper).
        let zone = Self::zone_at(temp);
        if zone != self.macro_state.last_zone {
            events::emit_event_json(&format!(
                r#"{{"kind":"zone_changed","from_zone":"{}","to_zone":"{}","crucible_temp":{}}}"#,
                self.macro_state.last_zone.as_str(),
                zone.as_str(),
                temp
            ));
            self.macro_state.last_zone = zone;
        }
        if temp > self.macro_state.params.fever_threshold_mk {
            let burn = fee * self.macro_state.params.fever_burn_bps as u128
                / constants::BPS_DENOMINATOR;
            self.macro_state.total_snacc_supply =
                self.macro_state.total_snacc_supply.saturating_sub(burn);
            self.macro_state.total_snacc_burned =
                self.macro_state.total_snacc_burned.saturating_add(burn);
            // `DK-033` (`ADR-28`): 50% fee non-burn ke kolam dividen, sisanya treasury.
            self.accrue_dividend_inflow(fee, burn);
        } else {
            self.accrue_dividend_inflow(fee, 0);
            if temp < self.macro_state.params.hypothermia_threshold_mk {
                let raw_rebate = fee * self.macro_state.params.rebate_bps as u128
                    / constants::BPS_DENOMINATOR;
                let max_epoch_drain = self.macro_state.epoch_treasury_initial
                    * self.macro_state.params.max_epoch_treasury_drain_bps as u128
                    / constants::BPS_DENOMINATOR;
                let remaining_quota =
                    max_epoch_drain.saturating_sub(self.macro_state.epoch_treasury_spent);
                let rebate = raw_rebate.min(remaining_quota).min(self.treasury);
                self.treasury = self.treasury.saturating_sub(rebate);
                if rebate > 0 {
                    let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
                    let _ = self
                        .snacc_balances
                        .insert(strain_id, bal.saturating_add(rebate));
                    self.macro_state.epoch_treasury_spent =
                        self.macro_state.epoch_treasury_spent.saturating_add(rebate);
                }
            }
        }
    }

    /// `DK-033` — `accrue_dividend_inflow(fee, burned)` (internal, Spec Dividend §4):
    /// `inflow = (fee - burned) × DIVIDEND_INFLOW_BPS / 10000` masuk `epoch_dividend_inflow`;
    /// sisanya (`net - inflow`) ke `Rule-Bound Treasury`. Kolam dividen terpisah dari
    /// kuota rebate `I-04` (Spec Dividend §3).
    fn accrue_dividend_inflow(&mut self, fee: u128, burned: u128) {
        let net = fee.saturating_sub(burned);
        let inflow = net * constants::DIVIDEND_INFLOW_BPS / constants::BPS_DENOMINATOR;
        self.treasury = self.treasury.saturating_add(net - inflow);
        self.macro_state.epoch_dividend_inflow =
            self.macro_state.epoch_dividend_inflow.saturating_add(inflow);
        events::emit_event_json(&format!(
            r#"{{"kind":"dividend_inflow","action":"catalyst_fee","fee":{},"burned":{},"inflow":{},"epoch":{}}}"#,
            fee,
            burned,
            inflow,
            self.macro_state.current_epoch
        ));
    }

    /// Pemulihan metabolik (`F2`): `vitality = min(10000, effective + 500)`, `last_fed_sec = now`.
    fn recover_metabolism(v: &mut StrainVessel, eff: u64, now_sec: u64) {
        v.vitality = std::cmp::min(
            constants::VITALITY_CAP,
            eff + constants::METABOLIC_RECOVERY_VITALITY,
        );
        v.last_fed_sec = now_sec;
    }

    fn inv_balance(&self, strain_id: StrainId, token: &str) -> u128 {
        self.inventories
            .get(&(strain_id, token.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// `mt_burn` — kurangi inventori (`ERR_INSUFFICIENT_BALANCE` bila kurang).
    fn inv_burn(&mut self, strain_id: StrainId, token: &str, amount: u128) {
        let key = (strain_id, token.to_string());
        let bal = self.inventories.get(&key).copied().unwrap_or(0);
        assert!(bal >= amount, "{}", constants::ERR_INSUFFICIENT_BALANCE);
        let _ = self.inventories.insert(key, bal - amount);
    }

    /// `mt_mint` — tambah inventori.
    fn inv_mint(&mut self, strain_id: StrainId, token: &str, amount: u128) {
        let key = (strain_id, token.to_string());
        let bal = self.inventories.get(&key).copied().unwrap_or(0);
        let _ = self.inventories.insert(key, bal + amount);
    }

    /// `F2` Tahap 1 — `extract_spores` (wajib `ROLE_EXTRACTOR`).
    /// Ambiguitas terdokumentasi: jumlah hasil ekstraksi tidak ternominalisasi di spec
    /// → **1 unit per panggilan** (keputusan review VC-004, 2026-11-07).
    #[payable]
    pub fn extract_spores(
        &mut self,
        strain_id: StrainId,
        spore_type: String,
        snacc_catalyst_fee: U128,
    ) -> StrainVesselView {
        let now_sec = env::block_timestamp() / 1_000_000_000;
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        let eff = Self::assert_not_starving(&v, now_sec);
        Self::assert_role(&v, constants::ROLE_EXTRACTOR);
        assert!(
            constants::SPORE_TOKEN_IDS.contains(&spore_type.as_str()),
            "{}",
            constants::ERR_UNKNOWN_TOKEN
        );
        self.take_catalyst_fee(strain_id, &snacc_catalyst_fee);
        self.inv_mint(strain_id, &spore_type, 1);
        Self::recover_metabolism(&mut v, eff, now_sec);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"extracted","strain_id":{},"token":"{}","amount":1,"catalyst_fee":{}}}"#,
            strain_id, spore_type, snacc_catalyst_fee.0
        ));
        self.get_strain(strain_id)
    }

    /// `F2` Tahap 2 — `transmute_spores_to_enzyme` (`mt_burn` spora → `mt_mint` enzim
    /// `floor(spores_to_burn * efficiency_bps / 10000)`, wajib `ROLE_FERMENTER`).
    /// Ambiguitas terdokumentasi: signature tanpa jenis spora/enzim → dipetakan ke
    /// **rasio resep per batch (2 spora)** dengan prioritas: amalgam → redactase → synthase.
    #[payable]
    pub fn transmute_spores_to_enzyme(
        &mut self,
        strain_id: StrainId,
        spores_to_burn: u64,
        snacc_catalyst_fee: U128,
    ) -> StrainVesselView {
        let now_sec = env::block_timestamp() / 1_000_000_000;
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        let eff = Self::assert_not_starving(&v, now_sec);
        Self::assert_role(&v, constants::ROLE_FERMENTER);
        assert!(
            spores_to_burn > 0 && spores_to_burn % 2 == 0,
            "{}",
            constants::ERR_RECIPE_NOT_MET
        );
        let burn = spores_to_burn as u128;
        let k = burn / 2; // batch resep = 2 spora (Katalog Molekul)
        let ignis = constants::SPORE_TOKEN_IDS[0];
        let cryo = constants::SPORE_TOKEN_IDS[1];
        let lumin = constants::SPORE_TOKEN_IDS[2];
        // Prioritas resep: `amalgam` (2x ignis) → `redactase` (1x ignis + 1x lumin) → `synthase` (2x cryo).
        let recipe: (&str, [(&str, u128); 2]) = if self.inv_balance(strain_id, ignis) >= burn {
            (constants::ENZYME_AMALGAM, [(ignis, burn), ("", 0)])
        } else if self.inv_balance(strain_id, ignis) >= k && self.inv_balance(strain_id, lumin) >= k {
            (constants::ENZYME_REDACTASE, [(ignis, k), (lumin, k)])
        } else if self.inv_balance(strain_id, cryo) >= burn {
            (constants::ENZYME_SYNTHASE, [(cryo, burn), ("", 0)])
        } else {
            env::panic_str(constants::ERR_INSUFFICIENT_BALANCE)
        };
        // `EN-110` — matriks diet per kasta per zona (peta koneksi: EN-04 → transmute).
        // Keputusan tercatat: Fever → Extractor −20%; Famine (Hypothermia) → Synthesizer
        // −20%; lainnya 0. Multi-role (hasil splice): cukup satu bit cocok.
        // `snacc_catalyst_fee` = nominal; attached deposit wajib = fee SETELAH diskon.
        let temp_now = self.get_crucible_temp();
        let diet_bps: u16 = match Self::zone_at(temp_now) {
            MacroZone::Fever if v.role_mask & constants::ROLE_EXTRACTOR != 0 => {
                constants::DIET_FEVER_EXTRACTOR_BPS
            }
            MacroZone::Hypothermia if v.role_mask & constants::ROLE_SYNTHESIZER != 0 => {
                constants::DIET_FAMINE_SYNTHESIZER_BPS
            }
            _ => 0,
        };
        let effective_fee =
            snacc_catalyst_fee.0 * u128::from(10000 - diet_bps) / constants::BPS_DENOMINATOR;
        self.take_catalyst_fee(strain_id, &U128(effective_fee));
        for (token, amount) in recipe.1 {
            if amount > 0 {
                self.inv_burn(strain_id, token, amount);
            }
        }
        let minted = burn * v.efficiency_bps as u128 / constants::BPS_DENOMINATOR;
        self.inv_mint(strain_id, recipe.0, minted);
        Self::recover_metabolism(&mut v, eff, now_sec);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"transmuted","strain_id":{},"burned_spores":{},"token_out":"{}","minted":{},"catalyst_fee":{},"diet_bps":{}}}"#,
            strain_id, spores_to_burn, recipe.0, minted, effective_fee, diet_bps
        ));
        self.get_strain(strain_id)
    }

    /// `F2` Tahap 3 — `synthesize_elixir` (wajib `ROLE_SYNTHESIZER`).
    /// `mt:elixir-aether` kini dapat disintesis bila strain **pernah sukses mutasi
    /// TEE** (`phenotype_trait.is_some()` — keputusan pipeline `DK-044`, 2026-11-07);
    /// TODO(F3): pertatkan gerbang ke attestation TEE per batch (spec belum ternominalisasi).
    /// Ambiguitas terdokumentasi: jumlah output eliksir tidak ternominalisasi →
    /// **1 eliksir per batch resep** (`enzymes_to_burn` harus kelipatan batch).
    #[payable]
    pub fn synthesize_elixir(
        &mut self,
        strain_id: StrainId,
        elixir_type: String,
        enzymes_to_burn: u64,
        snacc_catalyst_fee: U128,
    ) -> StrainVesselView {
        let now_sec = env::block_timestamp() / 1_000_000_000;
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        let eff = Self::assert_not_starving(&v, now_sec);
        Self::assert_role(&v, constants::ROLE_SYNTHESIZER);
        // Resep per batch (Katalog Molekul): (enzim_a, per_a, enzim_b, per_b, batch_total).
        // `mt:elixir-aether`: gerbang TEE (pernah sukses mutasi), resep = keputusan tercatat
        // (mengikuti bentuk resep homunculus: 2 amalgam + 1 synthase per batch).
        let recipe: (&str, u128, &str, u128, u128) = match elixir_type.as_str() {
            constants::ELIXIR_HOMUNCULUS => (
                constants::ENZYME_AMALGAM,
                2,
                constants::ENZYME_SYNTHASE,
                1,
                3,
            ),
            constants::ELIXIR_HOMEOSTASIS => (
                constants::ENZYME_AMALGAM,
                1,
                constants::ENZYME_REDACTASE,
                1,
                2,
            ),
            constants::ELIXIR_AETHER => {
                assert!(
                    v.phenotype_trait.is_some(),
                    "{}",
                    constants::ERR_REQUIRES_TEE_ATTESTATION
                );
                (constants::ENZYME_AMALGAM, 2, constants::ENZYME_SYNTHASE, 1, 3)
            }
            _ => env::panic_str(constants::ERR_UNKNOWN_TOKEN),
        };
        let (enz_a, per_a, enz_b, per_b, batch_total) = recipe;
        assert!(
            enzymes_to_burn > 0 && enzymes_to_burn as u128 % batch_total == 0,
            "{}",
            constants::ERR_RECIPE_NOT_MET
        );
        let batches = enzymes_to_burn as u128 / batch_total;
        let need_a = per_a * batches;
        let need_b = per_b * batches;
        assert!(
            self.inv_balance(strain_id, enz_a) >= need_a
                && self.inv_balance(strain_id, enz_b) >= need_b,
            "{}",
            constants::ERR_INSUFFICIENT_BALANCE
        );
        self.take_catalyst_fee(strain_id, &snacc_catalyst_fee);
        self.inv_burn(strain_id, enz_a, need_a);
        self.inv_burn(strain_id, enz_b, need_b);
        self.inv_mint(strain_id, &elixir_type, batches);
        Self::recover_metabolism(&mut v, eff, now_sec);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"synthesized","strain_id":{},"token_out":"{}","minted":{},"enzymes_burned":{},"catalyst_fee":{}}}"#,
            strain_id, elixir_type, batches, enzymes_to_burn, snacc_catalyst_fee.0
        ));
        self.get_strain(strain_id)
    }
}


// =====================================================================
// VC-006 — `F3`: TEE zero-trust 2-fase (`ADR-21`) + guardrail `I-03`
// =====================================================================
#[near]
impl SnaccContract {
    /// Dekode hex ke array tetap (`N` byte) — tanpa dependensi tambahan.
    fn hex_to_bytes<const N: usize>(s: &str) -> [u8; N] {
        assert_eq!(s.len(), N * 2, "{}", constants::ERR_BAD_HEX);
        let b = s.as_bytes();
        let nib = |c: u8| -> u8 {
            match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => env::panic_str(constants::ERR_BAD_HEX),
            }
        };
        let mut out = [0u8; N];
        for (i, item) in out.iter_mut().enumerate() {
            *item = (nib(b[2 * i]) << 4) | nib(b[2 * i + 1]);
        }
        out
    }

    fn hex_to_signature(s: &str) -> [u8; 64] {
        assert_eq!(s.len(), 128, "{}", constants::ERR_BAD_SIGNATURE_LEN);
        Self::hex_to_bytes::<64>(s)
    }

    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }

    /// Verifikasi tanda tangan enclave atas `<model>:<req_sha256>:<res_sha256>` (`S2.6`).
    /// Fungsi murni agar vektor signature **riil** dapat diuji langsung.
    fn verify_tee_receipt(
        req_sha256_hex: &str,
        res_sha256_hex: &str,
        signature_hex: &str,
        tee_pubkey_hex: &str,
    ) -> bool {
        let sig = Self::hex_to_signature(signature_hex);
        let pk = Self::hex_to_bytes::<32>(tee_pubkey_hex);
        let msg = format!(
            "{}:{}:{}",
            constants::TEE_MODEL_ID,
            req_sha256_hex,
            res_sha256_hex
        );
        env::ed25519_verify(&sig, msg.as_bytes(), &pk)
    }

    /// Parse `choices[0].message.content` + guardrail `I-03` (dipisah agar dapat diuji).
    fn parse_mutation_payload(raw_response_body: &str, strain_id: StrainId) -> MutationPayload {
        let envelope: ChatEnvelope = near_sdk::serde_json::from_str(raw_response_body)
            .unwrap_or_else(|_| {
                env::panic_str(constants::ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH)
            });
        let content = envelope
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        let payload: MutationPayload = near_sdk::serde_json::from_str(&content).unwrap_or_else(|_| {
            env::panic_str(constants::ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH)
        });
        assert_eq!(
            payload.strain_id, strain_id,
            "{}",
            constants::ERR_PAYLOAD_STRAIN_MISMATCH
        );
        assert!(
            payload.efficiency_shift_bps.abs() <= constants::TEE_SHIFT_LIMIT_BPS,
            "{}",
            constants::ERR_GUARDRAIL_SHIFT
        );
        assert!(
            payload.vitality_bonus <= constants::TEE_VITALITY_BONUS_CAP,
            "{}",
            constants::ERR_GUARDRAIL_VITALITY
        );
        payload
    }

    /// Kunci anti-replay: `chat_id`/`id` dari body; fallback `res_sha256` (unik per respons).
    /// Ambiguitas terdokumentasi: nama field id respons tidak disebut spec.
    fn extract_chat_key(raw_response_body: &str, res_sha256_hex: &str) -> String {
        if let Ok(envelope) = near_sdk::serde_json::from_str::<ChatEnvelope>(raw_response_body) {
            if let Some(id) = envelope.chat_id.or(envelope.id) {
                if !id.is_empty() {
                    return id;
                }
            }
        }
        res_sha256_hex.to_string()
    }

    /// `F3` / `P3` (`ADR-21`) — **Fase 1**: kunci komitmen `SHA-256` request
    /// (`commit_genome_experiment`) sebelum memanggil enclave TEE.
    pub fn commit_genome_experiment(&mut self, strain_id: StrainId, req_sha256_hex: String) {
        let owner = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(owner, v.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert!(
            v.pending_commitment.is_none(),
            "{}",
            constants::ERR_COMMITMENT_ALREADY_PENDING
        );
        let _ = Self::hex_to_bytes::<32>(&req_sha256_hex); // validasi format hex 32-byte
        v.pending_commitment = Some(req_sha256_hex.clone());
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"genome_committed","strain_id":{},"req_sha256_hex":"{}"}}"#,
            strain_id, req_sha256_hex
        ));
    }

    /// `F3` / `P3` (`ADR-21`, `I-03`, `I-16`) — **Fase 2**: `apply_tee_mutation_zero_trust`.
    ///
    /// Ambiguitas terdokumentasi (keputusan `VC-006`): parameter `tee_pubkey_hex`
    /// **ditambahkan** di luar tabel spec — verifikasi ke registri `attested_tee_signers`
    /// (`I-16`) tidak mungkin tanpa kunci pemverifikasi.
    pub fn apply_tee_mutation_zero_trust(
        &mut self,
        strain_id: StrainId,
        req_sha256_hex: String,
        raw_response_body: String,
        signature_hex: String,
        tee_pubkey_hex: String,
    ) -> StrainVesselView {
        let owner = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(owner, v.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        // Fase 1 binding: `pending_commitment` wajib cocok (`ADR-21`).
        assert_eq!(
            v.pending_commitment.as_deref(),
            Some(req_sha256_hex.as_str()),
            "{}",
            constants::ERR_COMMITMENT_MISMATCH
        );
        v.pending_commitment = None;
        // Hitung ulang `res_sha256` di on-chain (`ADR-21` — zero-trust).
        let res_sha256_hex = Self::hex_encode(&env::sha256(raw_response_body.as_bytes()));
        // `I-16` (`ADR-33`): kunci wajib terdaftar di `attested_tee_signers`.
        let pk = Self::hex_to_bytes::<32>(&tee_pubkey_hex);
        assert!(
            self.attested_tee_signers.get(&pk).is_some(),
            "{}",
            constants::ERR_SIGNER_NOT_REGISTERED
        );
        // Verifikasi tanda tangan enclave atas `<model>:<req_sha256>:<res_sha256>`.
        assert!(
            Self::verify_tee_receipt(
                &req_sha256_hex,
                &res_sha256_hex,
                &signature_hex,
                &tee_pubkey_hex
            ),
            "{}",
            constants::ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH
        );
        // Anti-replay (`D5`, `used_tee_chat_ids`).
        let chat_key = Self::extract_chat_key(&raw_response_body, &res_sha256_hex);
        assert!(
            !self.used_tee_chat_ids.contains(&chat_key),
            "{}",
            constants::ERR_TEE_REPLAY
        );
        let _ = self.used_tee_chat_ids.insert(chat_key.clone());
        // Parse payload + guardrail `I-03`, lalu terapkan mutasi ke Strain.
        let payload = Self::parse_mutation_payload(&raw_response_body, strain_id);
        v.efficiency_bps = (v.efficiency_bps as i64)
            .saturating_add(payload.efficiency_shift_bps)
            .clamp(0, u16::MAX as i64) as u16;
        v.vitality = std::cmp::min(
            constants::VITALITY_CAP,
            v.vitality.saturating_add(payload.vitality_bonus),
        );
        if let Some(trait_name) = payload.phenotype_trait.clone() {
            v.phenotype_trait = Some(trait_name);
        }
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        let _ = self.tee_attestations.insert(
            chat_key.clone(),
            TeeAttestation {
                chat_id: chat_key,
                model: constants::TEE_MODEL_ID.to_string(),
                req_sha256_hex,
                res_sha256_hex,
                signer_pubkey_hex: tee_pubkey_hex,
            },
        );
        events::emit_event_json(&format!(
            r#"{{"kind":"genome_mutated","strain_id":{},"efficiency_shift_bps":{},"vitality_bonus":{}}}"#,
            strain_id, payload.efficiency_shift_bps, payload.vitality_bonus
        ));
        self.get_strain(strain_id)
    }

    /// `ADR-33` / `I-16` — registri kunci publik enclave TEE (`register_attested_signer`).
    /// Ambiguitas terdokumentasi: spec tidak menyebut siapa yang mendaftarkan →
    /// bootstrap: penelepon pertama menjadi `owner_id`; TODO(SEC-113) pengerasan.
    pub fn register_attested_signer(&mut self, pubkey_hex: String, attested_at_sec: u64) {
        let caller = env::predecessor_account_id();
        if self.owner_id.is_none() {
            self.owner_id = Some(caller.clone());
        }
        assert_eq!(
            Some(&caller),
            self.owner_id.as_ref(),
            "{}",
            constants::ERR_NOT_CONTRACT_OWNER
        );
        let pk = Self::hex_to_bytes::<32>(&pubkey_hex);
        let _ = self.attested_tee_signers.insert(pk, attested_at_sec);
    }
}


// =====================================================================
// VC-007 — `F5`: `splice_strains` — Voluntary Coasean Merger (`ADR-25`)
// =====================================================================
#[near]
impl SnaccContract {
    /// `F5` / `P5` (`ADR-25`, `I-08`) — `splice_strains(dominant_id, absorbed_id)`.
    ///
    /// Pajak merger `2.5%` dari saldo `absorbed` ke `Rule-Bound Treasury`; sisa `97.5%`
    /// `$SNACC` + `100%` sembilan molekul `NEP-245` + `role_mask` diserap `dominant`
    /// (`dominant.role_mask |= absorbed.role_mask`, `I-08`), `generation + 1`,
    /// `efficiency_bps + 800`; `absorbed` -> `SplicedBurned`.
    /// TODO(EN-120): warisan set organel + wajib realokasi ATP; TODO(EN-124): cap `MAX_GENERATION`.
    pub fn splice_strains(
        &mut self,
        dominant_id: StrainId,
        absorbed_id: StrainId,
    ) -> StrainVesselView {
        assert_ne!(dominant_id, absorbed_id, "{}", constants::ERR_SAME_STRAIN);
        let caller = env::predecessor_account_id();
        let mut dom = self
            .strains
            .get(&dominant_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        let mut abs = self
            .strains
            .get(&absorbed_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, dom.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert_eq!(caller, abs.owner_id, "{}", constants::ERR_NOT_SAME_OWNER);
        assert!(
            !matches!(
                abs.status,
                StrainStatus::SplicedBurned | StrainStatus::Liquidated
            ),
            "{}",
            constants::ERR_STRAIN_ALREADY_SPLICED
        );
        // Pajak merger `2.5%` (`250 bps`) dari saldo `absorbed` (`ADR-25`).
        let abs_bal = self.snacc_balances.get(&absorbed_id).copied().unwrap_or(0);
        let tax = abs_bal * constants::MERGER_TAX_BPS as u128 / constants::BPS_DENOMINATOR;
        self.treasury = self.treasury.saturating_add(tax);
        let dom_bal = self.snacc_balances.get(&dominant_id).copied().unwrap_or(0);
        let _ = self
            .snacc_balances
            .insert(dominant_id, dom_bal.saturating_add(abs_bal - tax));
        let _ = self.snacc_balances.insert(absorbed_id, 0);
        // `100%` sembilan molekul `NEP-245` diserap ke `dominant`.
        for token in constants::ALL_TOKEN_IDS {
            let src = (absorbed_id, token.to_string());
            let amount = self.inventories.get(&src).copied().unwrap_or(0);
            if amount > 0 {
                let _ = self.inventories.insert(src, 0);
                let dst = (dominant_id, token.to_string());
                let cur = self.inventories.get(&dst).copied().unwrap_or(0);
                let _ = self.inventories.insert(dst, cur + amount);
            }
        }
        // Bitmask `I-08`, `Gen + 1`, `+800 bps` efisiensi.
        dom.role_mask |= abs.role_mask;
        dom.generation = dom.generation.saturating_add(1);
        dom.efficiency_bps = dom.efficiency_bps.saturating_add(800);
        abs.status = StrainStatus::SplicedBurned;
        dom.record_mutation();
        abs.record_mutation();
        let _ = self.strains.insert(dominant_id, dom);
        let _ = self.strains.insert(absorbed_id, abs);
        events::emit_event_json(&format!(
            r#"{{"kind":"strains_spliced","dominant_id":{},"absorbed_id":{},"merger_tax":{}}}"#,
            dominant_id, absorbed_id, tax
        ));
        self.get_strain(dominant_id)
    }
}


// =====================================================================
// TUGAS 2 #1 — `F5` Jalur 2: `hostile_takeover_splice` — Chapter 11 Hostile M&A
// (`ADR-26`, `I-09`, `I-14`, `ADR-29`)
// =====================================================================
#[near]
impl SnaccContract {
    /// `F5` Jalur 2 / `P5` (`ADR-26`, `I-09`, `I-14`) — `hostile_takeover_splice`.
    ///
    /// Cross-account: pemanggil wajib pemilik **raider** (`V-01`); target milik siapa pun.
    /// Sah bila `1 <= effective_vitality(target) <= 2000` (`Tier2Chapter11Distressed`,
    /// `ERR_NOT_DISTRESSED`) dan target tidak sudah Liquidated/SplicedBurned.
    /// `I-14` (`ADR-29`): `target.state_nonce == expected_target_nonce &&
    /// pending_callbacks == 0` (parameter `expected_target_nonce` ditambahkan sesuai
    /// `I-14`; `now_sec` dari spec §5.1 TIDAK diambil sebagai parameter — `V-02` waktu
    /// wajib dari blok). Anti-spoofing memakai `assert_atomic_ready` yang sudah ada.
    ///
    /// Pembayaran: `book_value = target.snacc_balance + Σ(inventori × MOLECULE_FLOOR_PRICE)`,
    /// raider membayar `book_value + 15%` (`HOSTILE_PREMIUM_BPS`); `95%` ke pemilik lama
    /// (`account_balances`), `5%` ke `Rule-Bound Treasury`. Raider menyerap 100% saldo +
    /// inventori + `role_mask |=`; `target.status = SplicedBurned`; efficiency = max dari
    /// keduanya (keputusan tercatat — spec hanya bilang "menggabungkan efisiensi").
    /// CATATAN DESAIN: margin langsung bisa negatif bila nilai pasar aset < 115% salvage —
    /// itu memang premium restrukturisasi; yang dibeli adalah `role_mask`, generasi, dan
    /// opsi perputaran aset, bukan arbitrase saldo.
    pub fn hostile_takeover_splice(
        &mut self,
        raider_strain_id: StrainId,
        distressed_target_id: StrainId,
        expected_target_nonce: u64,
    ) -> (U128, StrainVesselView) {
        assert_ne!(
            raider_strain_id, distressed_target_id,
            "{}",
            constants::ERR_SAME_STRAIN
        );
        let caller = env::predecessor_account_id();
        let mut raider = self
            .strains
            .get(&raider_strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, raider.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        let mut target = self
            .strains
            .get(&distressed_target_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert!(
            !matches!(
                target.status,
                StrainStatus::SplicedBurned | StrainStatus::Liquidated
            ),
            "{}",
            constants::ERR_STRAIN_ALREADY_SPLICED
        );
        // `I-09` / `ADR-26` — jendela Tier 2 Chapter 11 Distressed.
        let now_sec = env::block_timestamp() / 1_000_000_000;
        let eff = target.effective_vitality(now_sec);
        assert!(
            (1..=2000).contains(&eff),
            "{}",
            constants::ERR_NOT_DISTRESSED
        );
        // `I-14` (`ADR-29`) — anti ERC-6551 seller-drain (nonce + callback lock).
        target.assert_atomic_ready(expected_target_nonce);

        // Book value (`floor_price` = 1 per unit molekul — keputusan tercatat).
        let target_bal = self.snacc_balances.get(&distressed_target_id).copied().unwrap_or(0);
        let mut book_value = target_bal;
        for token in constants::ALL_TOKEN_IDS {
            book_value = book_value.saturating_add(
                self.inv_balance(distressed_target_id, token) * constants::MOLECULE_FLOOR_PRICE,
            );
        }
        let payment =
            book_value * (constants::BPS_DENOMINATOR + u128::from(constants::HOSTILE_PREMIUM_BPS))
                / constants::BPS_DENOMINATOR;
        let raider_bal = self.snacc_balances.get(&raider_strain_id).copied().unwrap_or(0);
        assert!(
            raider_bal >= payment,
            "{}",
            constants::ERR_INSUFFICIENT_BALANCE
        );

        // Split pembayaran: 95% ke pemilik lama, 5% ke treasury (`ADR-26`).
        let seller_payout = payment * constants::HOSTILE_SELLER_BPS / constants::BPS_DENOMINATOR;
        let treasury_cut = payment - seller_payout;
        let _ = self
            .snacc_balances
            .insert(raider_strain_id, raider_bal - payment + target_bal);
        let _ = self.snacc_balances.insert(distressed_target_id, 0);
        let seller = self
            .account_balances
            .get(&target.owner_id)
            .copied()
            .unwrap_or(0);
        let _ = self
            .account_balances
            .insert(target.owner_id.clone(), seller.saturating_add(seller_payout));
        self.treasury = self.treasury.saturating_add(treasury_cut);

        // Serap 100% inventori `NEP-245` + `role_mask |=` (`I-08`) + Gen+1 + efficiency max.
        for token in constants::ALL_TOKEN_IDS {
            let src = (distressed_target_id, token.to_string());
            let amount = self.inventories.get(&src).copied().unwrap_or(0);
            if amount > 0 {
                let _ = self.inventories.insert(src, 0);
                let dst = (raider_strain_id, token.to_string());
                let cur = self.inventories.get(&dst).copied().unwrap_or(0);
                let _ = self.inventories.insert(dst, cur + amount);
            }
        }
        raider.role_mask |= target.role_mask;
        raider.generation = raider.generation.saturating_add(1);
        raider.efficiency_bps = raider.efficiency_bps.max(target.efficiency_bps);
        target.status = StrainStatus::SplicedBurned;
        raider.record_mutation();
        target.record_mutation();
        let _ = self.strains.insert(raider_strain_id, raider.clone());
        let _ = self.strains.insert(distressed_target_id, target);

        // `D8` — log M&A.
        self.mna_logs.push(MnaLogEntry {
            timestamp_ms: env::block_timestamp() / 1_000_000,
            kind: state::MnaKind::HostileTakeover,
            actor_id: caller,
            target_id: distressed_target_id,
            amount: payment,
        });
        events::emit_event_json(&format!(
            r#"{{"kind":"hostile_takeover_spliced","raider_id":{},"target_id":{},"book_value":{},"payment":{},"seller_payout":{},"treasury_cut":{}}}"#,
            raider_strain_id, distressed_target_id, book_value, payment, seller_payout, treasury_cut
        ));
        (U128(payment), raider.to_view(now_sec))
    }
}


// =====================================================================
// VC-008 — `F6`: `harvest_starving_strain` — Cell Lysis (`ADR-11`, `I-10`)
// =====================================================================
#[near]
impl SnaccContract {
    /// `F6` / `P6` (`ADR-11`, `ADR-26`, `I-10`) — `harvest_starving_strain(target_id)`.
    ///
    /// Permissionless; sah hanya saat `effective_vitality(target) == 0` (`Tier3Chapter7Insolvent`).
    /// Split kas `90/10`: `10%` bounty `$SNACC` ke akun pemanggil, `90%` ke
    /// `Rule-Bound Treasury`. Semua inventori molekul dibakar; `status -> Liquidated`.
    /// TODO(EN-101/EN-102): `90%` bagian nutrient-burst akumulator; TODO(VC-009): SVG merah.
    /// Ambiguitas terdokumentasi: bounty dikreditkan ke `account_balances` akun pemanggil
    /// (spec F6: "dikreditkan ke akun pemanggil"; ledger NEP-141 global belum ada).
    pub fn harvest_starving_strain(&mut self, target_id: StrainId) -> (U128, U128) {
        let now_sec = env::block_timestamp() / 1_000_000_000;
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&target_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert!(
            !matches!(v.status, StrainStatus::Liquidated | StrainStatus::SplicedBurned),
            "{}",
            constants::ERR_ALREADY_LIQUIDATED
        );
        // `I-10`: hanya `Tier3Chapter7Insolvent` (`effective_vitality == 0`).
        assert_eq!(
            v.effective_vitality(now_sec),
            0,
            "{}",
            constants::ERR_STRAIN_NOT_INSOLVENT
        );
        let bal = self.snacc_balances.get(&target_id).copied().unwrap_or(0);
        let bounty = bal * constants::LIQUIDATOR_BOUNTY_BPS as u128 / constants::BPS_DENOMINATOR;
        let recovery = bal - bounty;
        // `10%` bounty ke akun pemanggil; `90%` ke `Rule-Bound Treasury`.
        let caller_bal = self.account_balances.get(&caller).copied().unwrap_or(0);
        let _ = self.account_balances.insert(caller.clone(), caller_bal + bounty);
        self.treasury = self.treasury.saturating_add(recovery);
        let _ = self.snacc_balances.insert(target_id, 0);
        // Inventori molekul dibakar habis (DK-054).
        for token in constants::ALL_TOKEN_IDS {
            let _ = self.inventories.insert((target_id, token.to_string()), 0);
        }
        v.status = StrainStatus::Liquidated;
        v.record_mutation();
        let _ = self.strains.insert(target_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"strain_liquidated","target_id":{},"liquidator":"{}","bounty":{},"treasury_recovery":{}}}"#,
            target_id, caller, bounty, recovery
        ));
        // `EN-102` — `Nutrient Burst`: 90% hasil likuidasi menghidupkan diskon feed
        // global selama 1 epoch. `scaled_share` = porsi `recovery` terhadap total
        // suplai (`keputusan tercatat`: spec hanya menyebut "scaled_share").
        let scaled_share =
            recovery * constants::BPS_DENOMINATOR
                / std::cmp::max(self.macro_state.total_snacc_supply, 1);
        let burst_bps = (u128::from(constants::BASE_BURST_BPS) + scaled_share)
            .min(u128::from(constants::MAX_BURST_BPS)) as u16;
        let until_epoch = self.macro_state.current_epoch + constants::BURST_DURATION_EPOCHS;
        self.macro_state.nutrient_burst_bps = burst_bps;
        self.macro_state.nutrient_burst_until = until_epoch;
        events::emit_event_json(&format!(
            r#"{{"kind":"nutrient_burst_released","strain_id":{},"epoch":{},"released_vita":{},"burst_bps":{},"until_epoch":{}}}"#,
            target_id, self.macro_state.current_epoch, recovery, burst_bps, until_epoch
        ));
        (U128(bounty), U128(recovery))
    }
}


// =====================================================================
// VC-009 — `F7`: `nft_token` — Dynamic Base64 SVG (`NEP-171`/`NEP-177`, `ADR-03`)
// =====================================================================
#[near]
impl SnaccContract {
    /// Zona makro dari suhu (kanonik: Fever `>1400`, Hypothermia `<600`, selain itu normal).
    fn zone_at(temp_mk: u64) -> MacroZone {
        if temp_mk > constants::FEVER_THRESHOLD_MK {
            MacroZone::Fever
        } else if temp_mk < constants::HYPOTHERMIA_THRESHOLD_MK {
            MacroZone::Hypothermia
        } else {
            MacroZone::Normal
        }
    }

    /// Nama zona untuk SVG/event (kanonik string `FEVER`/`HYPO`/`NORMAL`).
    fn zone_of(temp_mk: u64) -> &'static str {
        Self::zone_at(temp_mk).as_str()
    }

    /// `F7` / `P7` (`ADR-03`) — `nft_token(token_id)` → `NEP-171`/`NEP-177` view
    /// dengan `media = data:image/svg+xml;base64,...` berisi 8 indikator real-time
    /// + `role_mask` & `generation` (spike `S1.5`), merah `#ff2255` bila `Liquidated`.
    pub fn nft_token(&self, token_id: String) -> NftTokenView {
        let strain_id: StrainId = token_id
            .parse()
            .unwrap_or_else(|_| env::panic_str(constants::ERR_STRAIN_NOT_FOUND));
        let v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND);
        let now_ms = env::block_timestamp() / 1_000_000;
        let now_sec = now_ms / 1000;
        let balance = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        let bounty =
            balance * constants::LIQUIDATOR_BOUNTY_BPS as u128 / constants::BPS_DENOMINATOR;
        let temp = self.macro_state.crucible_temp_at(now_ms);
        let liquidated = matches!(
            v.status,
            StrainStatus::Liquidated | StrainStatus::SplicedBurned
        );
        let svg = svg::render_strain_svg(
            strain_id,
            v.role_mask,
            v.generation,
            balance,
            v.atp_balance,
            v.organel.mitokondria,
            v.organel.nukleus,
            v.organel.lisosom,
            now_sec.saturating_sub(v.last_fed_sec),
            bounty,
            temp,
            Self::zone_of(temp),
            liquidated,
        );
        NftTokenView {
            token_id,
            owner_id: v.owner_id.clone(),
            metadata: NftMetadataView {
                title: format!("Snacc Strain #{}", strain_id),
                description: format!(
                    "Living balance-sheet organism (role {}, gen {})",
                    v.role_mask, v.generation
                ),
                media: format!(
                    "data:image/svg+xml;base64,{}",
                    svg::base64_encode(svg.as_bytes())
                ),
            },
        }
    }
}

// =====================================================================
// N141-101..106 — surface standar `NEP-141` untuk coin `$SNACC`
// =====================================================================
#[near]
impl SnaccContract {
    /// Bantu: saldo `NEP-141` akun (ledger `account_balances`); 0 bila tak terdaftar.
    fn account_balance(&self, account: &AccountId) -> u128 {
        self.account_balances.get(account).copied().unwrap_or(0)
    }

    /// `N141-102` — keputusan tercatat: `ft_balance_of` = `account_balances` SAJA
    /// (uang di dalam Strain hanya bisa dipindah lewat mekanik game).
    pub fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.account_balance(&account_id))
    }

    /// `N141-102` — `ft_total_supply` = `macro_state.total_snacc_supply` (`I-01`).
    pub fn ft_total_supply(&self) -> U128 {
        U128(self.macro_state.total_snacc_supply)
    }

    /// `N141-102` — kekayaan total untuk display: `account_balances` + Σ saldo strain
    /// milik akun (indeks `strains_by_owner`).
    pub fn get_total_wealth(&self, account_id: AccountId) -> U128 {
        let mut total = self.account_balance(&account_id);
        if let Some(owned) = self.strains_by_owner.get(&account_id) {
            for strain_id in owned.iter() {
                total = total.saturating_add(
                    self.snacc_balances.get(strain_id).copied().unwrap_or(0),
                );
            }
        }
        U128(total)
    }

    /// `N141-101` — `ft_transfer` (`#[payable]`, wajib tepat 1 yoctoNEAR).
    /// Penerima wajib terdaftar storage (`ERR_ACCOUNT_NOT_REGISTERED`); saldo kurang
    /// `ERR_INSUFFICIENT_BALANCE`; self-transfer netral (debit lalu kredit akun sama).
    #[payable]
    pub fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        Self::assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        assert!(
            self.registered_storage.get(&receiver_id).is_some(),
            "{}",
            constants::ERR_ACCOUNT_NOT_REGISTERED
        );
        let sender_bal = self.account_balance(&sender_id);
        assert!(
            sender_bal >= amount.0,
            "{}",
            constants::ERR_INSUFFICIENT_BALANCE
        );
        if sender_id != receiver_id {
            let _ = self.account_balances.insert(sender_id.clone(), sender_bal - amount.0);
            let recv = self.account_balance(&receiver_id);
            let _ = self
                .account_balances
                .insert(receiver_id.clone(), recv.saturating_add(amount.0));
        }
        events::emit_standard_event(
            "nep141",
            "ft_transfer",
            &format!(
                r#"{{"old_owner_id":"{}","new_owner_id":"{}","amount":{},"memo":{}}}"#,
                sender_id,
                receiver_id,
                amount.0,
                match memo {
                    Some(m) => format!(r#""{}""#, m),
                    None => "null".to_string(),
                }
            ),
        );
    }

    fn assert_one_yocto() {
        assert_eq!(
            env::attached_deposit(),
            NearToken::from_yoctonear(1),
            "{}",
            constants::ERR_YOCTO_REQUIRED
        );
    }

    /// `N141-101` — `ft_transfer_call`: transfer + promise `receiver.ft_on_transfer`
    /// + callback `#[private] on_transfer_call` yang me-refund bagian tak terpakai.
    #[payable]
    pub fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> Promise {
        self.ft_transfer(receiver_id.clone(), amount, memo);
        let sender_id = env::predecessor_account_id();
        // Konvensi `NEP-141`: argumen `ft_on_transfer` = `{sender_id, amount, msg}`.
        let args = near_sdk::serde_json::to_vec(&near_sdk::serde_json::json!({
            "sender_id": sender_id,
            "amount": amount.0.to_string(),
            "msg": msg,
        }))
        .unwrap();
        Promise::new(receiver_id.clone())
            .function_call("ft_on_transfer".to_string(), args, NearToken::from_near(0),
                near_sdk::Gas::from_tgas(20))
            .then(Self::ext(env::current_account_id()).on_transfer_call(
                sender_id,
                receiver_id,
                amount,
            ))
    }

    /// `ADR-24` — callback anti-spoofing: refund bagian `ft_on_transfer` yang tidak
    /// dipakai ke pengirim; bila promise gagal, refund penuh.
    #[private]
    pub fn on_transfer_call(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let unused = match env::promise_result_checked(0, usize::MAX) {
            Ok(bytes) => near_sdk::serde_json::from_slice::<U128>(&bytes)
                .map(|u| u.0)
                .unwrap_or(0)
                .min(amount.0),
            Err(_) => amount.0, // gagal `ft_on_transfer` -> refund penuh
        };
        if unused > 0 {
            let bal = self.account_balance(&sender_id);
            let _ = self
                .account_balances
                .insert(sender_id.clone(), bal.saturating_add(unused));
            // Sisa dana ditarik kembali dari penerima (mengembalikan konservasi).
            let recv_bal = self.account_balance(&receiver_id);
            let _ = self
                .account_balances
                .insert(receiver_id, recv_bal.saturating_sub(unused));
        }
        U128(unused)
    }

    /// `N141-103` — metadata `NEP-141`. `decimals: 24` = keputusan tercatat.
    pub fn ft_metadata(&self) -> FungibleTokenMetadata {
        let logo = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect width=\"64\" height=\"64\" fill=\"#1b0f2e\"/><text x=\"32\" y=\"43\" font-family=\"monospace\" font-size=\"30\" fill=\"#ff8a3d\" text-anchor=\"middle\">S</text></svg>";
        FungibleTokenMetadata {
            spec: "ft-1.0.0".to_string(),
            name: "Snacc".to_string(),
            symbol: "$SNACC".to_string(),
            decimals: 24,
            icon: Some(format!(
                "data:image/svg+xml;base64,{}",
                svg::base64_encode(logo.as_bytes())
            )),
            reference: None,
            reference_hash: None,
        }
    }

    /// `N141-104` — `storage_deposit` MVP: registry akun -> total deposit (milliNEAR).
    #[payable]
    pub fn storage_deposit(&mut self, account_id: Option<AccountId>) -> StorageBalance {
        let target = account_id.unwrap_or_else(env::predecessor_account_id);
        let attached = env::attached_deposit().as_yoctonear();
        let min_yocto =
            NearToken::from_millinear(constants::STORAGE_DEPOSIT_MILLINEAR).as_yoctonear();
        let existing = self.registered_storage.get(&target).copied().unwrap_or(0);
        if existing == 0 {
            // Minimum 5 milliNEAR hanya untuk registrasi pertama (keputusan tercatat).
            assert!(
                attached >= min_yocto,
                "{}",
                constants::ERR_INSUFFICIENT_DEPOSIT
            );
        }
        let total = existing.saturating_add(attached);
        let _ = self.registered_storage.insert(target.clone(), total);
        StorageBalance {
            total: U128(total),
            available: U128(total),
        }
    }

    /// `N141-104` — `storage_withdraw`: tarik deposit berlebih. Saldo minimum
    /// (`STORAGE_DEPOSIT_MILLINEAR`) tetap terkunci selama akun memegang `$SNACC > 0`
    /// (keputusan tercatat, MVP).
    #[payable]
    pub fn storage_withdraw(&mut self, amount: Option<U128>) -> StorageBalance {
        let caller = env::predecessor_account_id();
        let total = self
            .registered_storage
            .get(&caller)
            .copied()
            .unwrap_or(0);
        assert!(total > 0, "{}", constants::ERR_ACCOUNT_NOT_REGISTERED);
        let locked = if self.account_balance(&caller) > 0 {
            NearToken::from_millinear(constants::STORAGE_DEPOSIT_MILLINEAR).as_yoctonear()
        } else {
            0
        };
        let available = total.saturating_sub(locked);
        let withdraw = match amount {
            Some(a) => {
                assert!(
                    a.0 <= available,
                    "{}",
                    constants::ERR_INSUFFICIENT_BALANCE
                );
                a.0
            }
            None => available,
        };
        if withdraw > 0 {
            let new_total = total - withdraw;
            if new_total == 0 {
                // Menarik penuh = batal registrasi (tak bisa menerima transfer lagi).
                let _ = self.registered_storage.remove(&caller);
            } else {
                let _ = self.registered_storage.insert(caller.clone(), new_total);
            }
            let _ = Promise::new(caller).transfer(NearToken::from_yoctonear(withdraw));
        }
        let new_total = total - withdraw;
        StorageBalance {
            total: U128(new_total),
            available: U128(new_total.saturating_sub(locked)),
        }
    }

    /// `N141-104` — `storage_balance_of`.
    pub fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.registered_storage
            .get(&account_id)
            .copied()
            .map(|total| StorageBalance {
                total: U128(total),
                available: U128(total),
            })
    }
}

/// Metadata `NEP-141` (`N141-103`) — subset standar + ikon SVG inline.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct FungibleTokenMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub icon: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

/// `N141-104` — saldo storage MVP.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct StorageBalance {
    pub total: U128,
    pub available: U128,
}

// =====================================================================
// DK-033 / DK-044 — Industrial Dividend Pool (`ADR-28`)
// =====================================================================
#[near]
impl SnaccContract {
    /// `DK-044` — bakar `mt:elixir-aether` sebagai kontribusi dividen epoch berjalan
    /// (Fase 1 pro-rata; belum membayar sampai epoch ditutup).
    pub fn synthesize_and_redeem_aether(&mut self, strain_id: StrainId, amount: U128) {
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        // `V-01` — hanya pemilik.
        assert_eq!(caller, v.owner_id, "{}", constants::ERR_NOT_STRAIN_OWNER);
        // Gerbang kasta: butuh bit `ROLE_SYNTHESIZER`.
        assert!(
            v.role_mask & constants::ROLE_SYNTHESIZER != 0,
            "{}",
            constants::ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE
        );
        assert!(
            amount.0 >= constants::MIN_AETHER_BURN,
            "{}",
            constants::ERR_INSUFFICIENT_AETHER
        );
        // Stok `mt:elixir-aether` cukup.
        let stock = self.inv_balance(strain_id, constants::ELIXIR_AETHER);
        assert!(
            stock >= amount.0,
            "{}",
            constants::ERR_INSUFFICIENT_AETHER
        );
        let now_sec = env::block_timestamp() / 1_000_000_000;
        // `effective_vitality > 0` (`ERR_STRAIN_STARVING`).
        let _ = Self::assert_not_starving(&v, now_sec);

        self.inv_burn(strain_id, constants::ELIXIR_AETHER, amount.0);
        let epoch = self.macro_state.current_epoch;
        let key = (epoch, strain_id);
        let prev = self.epoch_aether_burned.get(&key).copied().unwrap_or(0);
        let _ = self.epoch_aether_burned.insert(key, prev.saturating_add(amount.0));
        self.macro_state.epoch_aether_burned_total = self
            .macro_state
            .epoch_aether_burned_total
            .saturating_add(amount.0);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"aether_burned_for_dividend","strain_id":{},"amount":{},"epoch":{},"cumulative":{}}}"#,
            strain_id,
            amount.0,
            epoch,
            self.macro_state.epoch_aether_burned_total
        ));
    }

    /// `DK-033` Fase 2 — klaim dividen pro-rata untuk `(strain_id, epoch)` yang sudah
    /// ditutup: `payout = kontribusi × epoch_dividend_rate[epoch] / DIVIDEND_RATE_SCALE`,
    /// dipotong ke saldo kolam (tidak pernah mint — `DK-010`), sekali per `(epoch, strain)`.
    pub fn claim_industrial_dividend(&mut self, strain_id: StrainId, epoch: Epoch) -> U128 {
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, v.owner_id, "{}", constants::ERR_NOT_STRAIN_OWNER);
        // Epoch wajib sudah ditutup.
        assert!(
            epoch < self.macro_state.last_closed_epoch,
            "{}",
            constants::ERR_EPOCH_NOT_CLOSED
        );
        // Batas klaim mundur `MAX_DIVIDEND_CLAIM_EPOCHS`.
        assert!(
            epoch
                >= self
                    .macro_state
                    .current_epoch
                    .saturating_sub(constants::MAX_DIVIDEND_CLAIM_EPOCHS),
            "{}",
            constants::ERR_DIVIDEND_CLAIM_EXPIRED
        );
        let claim_key = (epoch, strain_id);
        assert!(
            !self.dividend_claimed.contains(&claim_key),
            "{}",
            constants::ERR_DIVIDEND_ALREADY_CLAIMED
        );
        let contribution = self.epoch_aether_burned.get(&claim_key).copied().unwrap_or(0);
        let rate = self.epoch_dividend_rate.get(&epoch).copied().unwrap_or(0);
        let mut payout = contribution
            .saturating_mul(rate)
            / constants::DIVIDEND_RATE_SCALE;
        // Cap: payout ≤ saldo kolam (menutup `V-04`); kolam kosong -> error eksplisit.
        if payout > self.macro_state.industrial_dividend_pool {
            payout = self.macro_state.industrial_dividend_pool;
        }
        assert!(
            payout > 0,
            "{}",
            constants::ERR_DIVIDEND_POOL_EMPTY
        );
        self.macro_state.industrial_dividend_pool = self
            .macro_state
            .industrial_dividend_pool
            .saturating_sub(payout);
        let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        let _ = self.snacc_balances.insert(strain_id, bal.saturating_add(payout));
        let _ = self.dividend_claimed.insert(claim_key);
        // `ADR-29` — setiap mutasi saldo menaikkan nonce.
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"dividend_claimed","strain_id":{},"epoch":{},"contribution":{},"payout":{},"pool_after":{}}}"#,
            strain_id,
            epoch,
            contribution,
            payout,
            self.macro_state.industrial_dividend_pool
        ));
        U128(payout)
    }

    /// `DK-033` — view kolam: `(industrial_dividend_pool, epoch_dividend_inflow, last_closed_epoch)`.
    pub fn get_dividend_pool(&self) -> (U128, U128, Epoch) {
        (
            U128(self.macro_state.industrial_dividend_pool),
            U128(self.macro_state.epoch_dividend_inflow),
            self.macro_state.last_closed_epoch,
        )
    }

    /// `DK-033` — rate epoch (skala `DIVIDEND_RATE_SCALE`); 0 bila belum ditutup.
    pub fn get_epoch_dividend_rate(&self, epoch: Epoch) -> U128 {
        U128(self.epoch_dividend_rate.get(&epoch).copied().unwrap_or(0))
    }

    /// `DK-033` — kontribusi `(epoch, strain)` + status klaim.
    pub fn get_dividend_contribution(&self, epoch: Epoch, strain_id: StrainId) -> (U128, bool) {
        (
            U128(self
                .epoch_aether_burned
                .get(&(epoch, strain_id))
                .copied()
                .unwrap_or(0)),
            self.dividend_claimed.contains(&(epoch, strain_id)),
        )
    }
}

// =====================================================================
// EN-101..104 — Nutrient Burst (spec Enhancement §3)
// =====================================================================
#[near]
impl SnaccContract {
    /// `EN-101` — view state burst: `(nutrient_burst_bps_aktif, nutrient_burst_until)`.
    /// `bps` yang dikembalikan adalah nilai EFEKTIF untuk epoch berjalan (0 bila
    /// kedaluwarsa — tanpa keeper, `ADR-27`).
    pub fn get_burst_state(&self) -> (U128, Epoch) {
        let epoch = self.macro_state.current_epoch;
        (
            U128(u128::from(self.macro_state.active_burst_bps(epoch))),
            self.macro_state.nutrient_burst_until,
        )
    }

    /// `EN-103` — `feed_strain(strain_id, amount)`: bakar `$SNACC` dari saldo strain
    /// sendiri sebagai makanan. Keputusan tercatat (spec tidak ternominalisasi):
    /// `base_fee = amount`; porsi yang BENAR-BENAR keluar dari saldo strain adalah
    /// `effective_fee = base_fee × (10000 − burst) / 10000` saat burst aktif, lalu
    /// masuk jalur termostat `F4` (volume, Fever burn, rebate `I-04`, split dividen
    /// `DK-033`); sisa diskon tetap di saldo strain (di sinilah "feed lebih murah").
    /// Pemulihan metabolik `+500` & `last_fed_sec = now` (pola `F2`).
    /// Modifier lanjutan (urutan: burst → rescue → dormansi, semuanya memperkecil fee):
    /// - `EN-109` rescue feeding: multiplier linear `RESCUE_MIN_FEE_BPS..=10000` atas
    ///   `effective_vitality` — makin sekarat makin murah (batas 0 → 20%, 100% → penuh).
    /// - `EN-108` dormansi: saat `decayed_volume < ambang` (`DORMANCY_THRESHOLD_BPS`,
    ///   keputusan tercatat 1% supply) upkeep turun ke `UPKEEP_MIN_BPS = 50` bps.
    /// - `EN-105` volume feed masuk akumulator Great Feast + leaderboard top-3.
    pub fn feed_strain(&mut self, strain_id: StrainId, amount: U128) -> StrainVesselView {
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, v.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert!(amount.0 > 0, "{}", constants::ERR_INSUFFICIENT_BALANCE);
        let now_sec = env::block_timestamp() / 1_000_000_000;
        let now_ms = env::block_timestamp() / 1_000_000;
        let eff = Self::assert_not_starving(&v, now_sec);
        let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        assert!(
            bal >= amount.0,
            "{}",
            constants::ERR_INSUFFICIENT_BALANCE
        );

        // `EN-103` — diskon burst, dengan cap `EN-104` (5% Pantry/epoch, kuota `I-04`).
        let base_fee = amount.0;
        let burst_bps = self.macro_state.active_burst_bps(self.macro_state.current_epoch);
        let raw_discount = base_fee - base_fee * u128::from(10000 - burst_bps)
            / constants::BPS_DENOMINATOR;
        let max_epoch_drain = self.macro_state.epoch_treasury_initial
            * self.macro_state.params.max_epoch_treasury_drain_bps as u128
            / constants::BPS_DENOMINATOR;
        let remaining_quota = max_epoch_drain
            .saturating_sub(self.macro_state.epoch_treasury_spent)
            .saturating_sub(self.macro_state.epoch_burst_discount_granted);
        let discount = raw_discount.min(remaining_quota);
        self.macro_state.epoch_burst_discount_granted = self
            .macro_state
            .epoch_burst_discount_granted
            .saturating_add(discount);

        // `EN-109` — rescue feeding: multiplier linear atas `effective_vitality`.
        let rescue_bps = constants::RESCUE_MIN_FEE_BPS
            + (constants::BPS_DENOMINATOR - constants::RESCUE_MIN_FEE_BPS)
                * u128::from(eff)
                / u128::from(constants::VITALITY_CAP as u16);

        // `EN-108` — dormansi: upkeep turun ke `UPKEEP_MIN_BPS` saat volume di bawah ambang.
        let dormancy_threshold = self.macro_state.total_snacc_supply
            * u128::from(self.macro_state.params.dormancy_threshold_bps)
            / constants::BPS_DENOMINATOR;
        let dormant =
            self.macro_state.decayed_volume(now_ms) < dormancy_threshold;
        let upkeep_bps = if dormant {
            constants::UPKEEP_MIN_BPS
        } else {
            constants::BPS_DENOMINATOR
        };

        let effective_fee = (base_fee - discount) * rescue_bps / constants::BPS_DENOMINATOR
            * upkeep_bps
            / constants::BPS_DENOMINATOR;

        // Debit `effective_fee` dari saldo strain -> jalur termostat `F4`.
        let _ = self
            .snacc_balances
            .insert(strain_id, bal - effective_fee);
        self.apply_thermostat_fee(strain_id, effective_fee);

        // `EN-105` — akumulator Great Feast + leaderboard top-3 (`EN-107`).
        let epoch = self.macro_state.current_epoch;
        self.macro_state.epoch_feast_volume = self
            .macro_state
            .epoch_feast_volume
            .saturating_add(effective_fee);
        let contrib = self
            .feast_contributions
            .get(&(epoch, caller.clone()))
            .copied()
            .unwrap_or(0)
            .saturating_add(effective_fee);
        let _ = self.feast_contributions.insert((epoch, caller.clone()), contrib);
        self.feast_update_top3(epoch, &caller, contrib);

        // Pemulihan metabolik (pola `F2`) + nonce.
        Self::recover_metabolism(&mut v, eff, now_sec);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"fed","strain_id":{},"base_fee":{},"effective_fee":{},"burst_bps":{},"rescue_bps":{},"dormant":{}}}"#,
            strain_id, base_fee, effective_fee, burst_bps, rescue_bps, dormant
        ));
        self.get_strain(strain_id)
    }

    /// `EN-107` — pemeliharaan leaderboard top-3 feeder (batas 3 entri, gas aman).
    fn feast_update_top3(&mut self, epoch: Epoch, feeder: &AccountId, amount: u128) {
        let mut top = self.epoch_top_feeders.get(&epoch).cloned().unwrap_or_default();
        match top.iter_mut().find(|e| &e.account_id == feeder) {
            Some(entry) => entry.amount = amount,
            None => top.push(FeederEntry {
                account_id: feeder.clone(),
                amount,
            }),
        }
        top.sort_by(|a, b| b.amount.cmp(&a.amount));
        top.truncate(3);
        let _ = self.epoch_top_feeders.insert(epoch, top);
    }

    /// `EN-107` — leaderboard top-3 feeder untuk satu epoch (maksimal 3 entri).
    pub fn get_top_feeder(&self, epoch: Epoch) -> Vec<FeederEntry> {
        self.epoch_top_feeders.get(&epoch).cloned().unwrap_or_default()
    }

    /// `EN-112` — `get_appetite_meter()`: zona makro, suhu, countdown burst,
    /// status dormansi, dan progres Great Feast (view spec `EN-05`).
    pub fn get_appetite_meter(&self) -> AppetiteMeterView {
        let now_ms = env::block_timestamp() / 1_000_000;
        let temp = self.macro_state.crucible_temp_at(now_ms);
        let zone = Self::zone_at(temp);
        let epoch = self.macro_state.current_epoch;
        let feast_target = self.macro_state.total_snacc_supply * constants::FEAST_TARGET_BPS
            / constants::BPS_DENOMINATOR;
        let dormancy_threshold = self.macro_state.total_snacc_supply
            * u128::from(self.macro_state.params.dormancy_threshold_bps)
            / constants::BPS_DENOMINATOR;
        AppetiteMeterView {
            zone: zone.as_str().to_string(),
            crucible_temp: temp,
            current_epoch: epoch,
            burst_bps: self.macro_state.active_burst_bps(epoch),
            burst_until: self.macro_state.nutrient_burst_until,
            burst_epochs_left: self.macro_state.nutrient_burst_until.saturating_sub(epoch),
            dormant: self.macro_state.decayed_volume(now_ms) < dormancy_threshold,
            feast_volume: self.macro_state.epoch_feast_volume,
            feast_target,
        }
    }
}

/// `EN-112` — tampilan `get_appetite_meter()`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct AppetiteMeterView {
    /// Zona makro (`NORMAL`/`FEVER`/`HYPO`).
    pub zone: String,
    pub crucible_temp: u64,
    pub current_epoch: Epoch,
    /// Diskon burst aktif epoch berjalan (0 bila kedaluwarsa).
    pub burst_bps: u16,
    pub burst_until: Epoch,
    /// Countdown burst dalam epoch (`burst_until − current_epoch`).
    pub burst_epochs_left: Epoch,
    /// `EN-108` — status dormansi (volume < ambang).
    pub dormant: bool,
    /// `EN-105` — progres Great Feast epoch berjalan.
    pub feast_volume: u128,
    pub feast_target: u128,
}


// =====================================================================
// Migrasi state — WAJIB dipanggil sekali setelah redeploy (borsh posisional)
// =====================================================================
#[near]
impl SnaccContract {
    /// Migrasi state NEAR (pola standar): `#[init(ignore_state)]` berjalan TANPA
    /// memuat state lama. Semua collection memakai prefix byte yang sama dengan
    /// `new()`, sehingga data trie lama (hidup di storage key ber-prefix) tetap
    /// terbaca — hanya blob state (borsh) yang diganti dengan layout baru.
    /// Tanpa ini, redeploy ke akun ber-state lama akan gagal deserialisasi
    /// (field baru di tengah/tengah layout) = seluruh view/call panic.
    #[init(ignore_state)]
    pub fn migrate() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::serde_json;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    /// Konteks test resmi near-sdk 5 (`testing_env!` + `VMContextBuilder`).
    fn set_context(predecessor: &str, deposit: NearToken) {
        set_context_at(predecessor, deposit, 3_600_000_000_000); // 1 jam — Lazy BMR terkendali
    }

    fn set_context_at(predecessor: &str, deposit: NearToken, block_timestamp: u64) {
        let mut builder = VMContextBuilder::new();
        builder
            .predecessor_account_id(predecessor.parse().unwrap())
            .attached_deposit(deposit)
            .block_timestamp(block_timestamp);
        testing_env!(builder.build());
    }

    #[test]
    fn smoke_scaffold() {
        let c = SnaccContract::new();
        assert_eq!(c.protocol_version(), "snacc-v001");
        assert_eq!(c.treasury, 0);
        assert_eq!(c.macro_state.current_epoch, 0);
    }

    // ----------------------------------------------------------------------
    // VC-003 — F1: incubate_strain / materialize_subaccount / get_strain
    // ----------------------------------------------------------------------

    const ALKEMIS_ID: &str = "alkemis.testnet";
    const EXTRACTOR_ROLE: &str = "Extractor";
    const FERMENTER_ROLE: &str = "Fermenter";
    const SYNTHESIZER_ROLE: &str = "Synthesizer";

    fn role(name: &str) -> state::StrainRole {
        match name {
            "Extractor" => state::StrainRole::Extractor,
            "Fermenter" => state::StrainRole::Fermenter,
            _ => state::StrainRole::Synthesizer,
        }
    }

    // Helper: panggil incubate_strain dengan attached deposit == stake.
    fn call_incubate(c: &mut SnaccContract, stake: u128, role_name: &str, spores: u64) -> state::StrainVesselView {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(stake));
        c.incubate_strain(role(role_name), U128(stake), spores)
    }

    /// (a) 80/20 split tepat: stake=1 NEAR -> strain=8e11, treasury=2e11.
    #[test]
    fn vc003_split_80_20_tepat() {
        let mut c = SnaccContract::new();
        let stake = 1_000_000_000_000u128; // 1e12 yocto = 1 NEAR
        let view = call_incubate(&mut c, stake, EXTRACTOR_ROLE, 5);
        assert_eq!(view.strain_id, 1);
        assert_eq!(c.snacc_balances.get(&1), Some(&800_000_000_000));
        assert_eq!(c.treasury, 200_000_000_000);
    }

    /// (a) argumen 0: nominal diambil dari attached deposit.
    #[test]
    fn vc003_argumen_0_ambil_attached_deposit() {
        let mut c = SnaccContract::new();
        let stake = 500_000_000_000u128;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(stake));
        let view = c.incubate_strain(role(EXTRACTOR_ROLE), U128(0), 3);
        assert_eq!(view.strain_id, 1);
        assert_eq!(c.snacc_balances.get(&1), Some(&400_000_000_000)); // 80%
        assert_eq!(c.treasury, 100_000_000_000); // 20%
    }

    /// (a) deposit < nominal argumen -> ditolak.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn vc003_deposit_mismatch_ditolak() {
        let mut c = SnaccContract::new();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(100));
        let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(1_000_000_000_000), 5);
    }

    /// (b) BMR per kasta: Extractor=100, Fermenter=120, Synthesizer=150.
    #[test]
    fn vc003_bmr_per_kasta() {
        let mut c = SnaccContract::new();
        let e = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(e.bmr_per_hour, 100);
        let f = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        assert_eq!(f.bmr_per_hour, 120);
        let s = call_incubate(&mut c, 1_000_000_000_000u128, SYNTHESIZER_ROLE, 0);
        assert_eq!(s.bmr_per_hour, 150);
    }

    /// (c) vitality=8500 & efficiency=10000 pada semua kasta.
    #[test]
    fn vc003_vitality_8500_efficiency_10000() {
        let mut c = SnaccContract::new();
        let e = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(e.vitality, 8500);
        assert_eq!(e.efficiency_bps, 10000);
        let f = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        assert_eq!(f.vitality, 8500);
        assert_eq!(f.efficiency_bps, 10000);
        let s = call_incubate(&mut c, 1_000_000_000_000u128, SYNTHESIZER_ROLE, 0);
        assert_eq!(s.vitality, 8500);
        assert_eq!(s.efficiency_bps, 10000);
    }

    /// (d) sub-account `s-{id}.snacc-v001.near`.
    #[test]
    fn vc003_subaccount_format() {
        let mut c = SnaccContract::new();
        let v1 = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(v1.vault_subaccount.to_string(), "s-1.snacc-v001.near");
        let v2 = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        assert_eq!(v2.vault_subaccount.to_string(), "s-2.snacc-v001.near");
    }

    /// (e) spora termint ke inventori NEP-245 (strain_id, token_id).
    #[test]
    fn vc003_spora_termint() {
        let mut c = SnaccContract::new();
        call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 7);
        let key = (1u64, "mt:spore-ignis".to_string());
        assert_eq!(c.inventories.get(&key), Some(&7));
        assert_eq!(state::constants::GENESIS_SPORE_TOKEN_ID, "mt:spore-ignis");
    }

    /// (f) get_strain JSON benar — `StrainVesselView` deserialisasi JSON.
    #[test]
    fn vc003_get_strain_json() {
        let mut c = SnaccContract::new();
        let stake = 1_000_000_000_000u128;
        let _ = call_incubate(&mut c, stake, EXTRACTOR_ROLE, 5);

        let view0 = c.get_strain(1);
        let json = serde_json::to_string(&view0).unwrap();
        let back: state::StrainVesselView = serde_json::from_str(&json).unwrap();
        assert_eq!(view0, back);

        let view = c.get_strain(1);
        assert_eq!(view.strain_id, 1);
        assert_eq!(view.owner_id.to_string(), ALKEMIS_ID);
        assert_eq!(view.vault_subaccount.to_string(), "s-1.snacc-v001.near");
        assert_eq!(view.vitality, 8500);
        assert_eq!(view.efficiency_bps, 10000);
        assert_eq!(view.bmr_per_hour, 100);
        assert_eq!(view.state_nonce, 0);
        assert_eq!(view.pending_callbacks, 0);
        assert_eq!(view.atp_balance, 0);
        assert_eq!(view.generation, 0);
        assert_eq!(view.status, state::StrainStatus::Active);
        assert_eq!(view.role_mask, state::constants::ROLE_EXTRACTOR);
        assert_eq!(view.effective_vitality, 8500);
    }

    /// (g) deposit < 0.02 NEAR ditolak pada materialize_subaccount.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn vc003_materialize_deposit_kurang_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.materialize_subaccount(1);
    }

    /// (h) non-owner ditolak pada materialize_subaccount.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn vc003_non_owner_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_near(1));
        let _ = c.materialize_subaccount(1);
    }

    /// (e) strain_id berurutan mulai 1.
    #[test]
    fn vc003_sequential_strain_id() {
        let mut c = SnaccContract::new();
        let v1 = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        let v2 = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        let v3 = call_incubate(&mut c, 1_000_000_000_000u128, SYNTHESIZER_ROLE, 0);
        assert_eq!(v1.strain_id, 1);
        assert_eq!(v2.strain_id, 2);
        assert_eq!(v3.strain_id, 3);
    }

    /// (e) AlkemisProfile: joined_sec saat pertama, strain_count += 1.
    #[test]
    fn vc003_alkemis_profile_updated() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        let profile = c.alchemists.get(&ALKEMIS_ID.parse::<AccountId>().unwrap()).unwrap();
        assert_eq!(profile.strain_count, 2);
        assert!(profile.joined_sec > 0);
        assert_eq!(profile.account_id.to_string(), ALKEMIS_ID);
    }

    /// (e) StrainRole hanya punya 3 varian, masing-masing single bit.
    #[test]
    fn vc003_strain_role_enum() {
        use state::StrainRole;
        assert_eq!(StrainRole::Extractor.to_role_mask(), 0b001);
        assert_eq!(StrainRole::Fermenter.to_role_mask(), 0b010);
        assert_eq!(StrainRole::Synthesizer.to_role_mask(), 0b100);
    }

    /// get_strain strain_id tidak terdaftar -> ditolak.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn vc003_get_strain_not_found() {
        let c = SnaccContract::new();
        let _ = c.get_strain(42);
    }

    /// (a) argumen 0 tanpa deposit -> ditolak.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn vc003_argumen_0_tanpa_deposit() {
        let mut c = SnaccContract::new();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(0), 0);
    }

    // ----------------------------------------------------------------------
    // VC-004 — F2: extract_spores / transmute_spores_to_enzyme / synthesize_elixir
    // ----------------------------------------------------------------------

    fn grant(c: &mut SnaccContract, token: &str, amount: u128) {
        let _ = c.inventories.insert((1, token.to_string()), amount);
    }

    /// (a) Lazy BMR: vitality habis -> `ERR_STRAIN_STARVING`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn vc004_starving_gate_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        // 200 jam x BMR 100/jam = 20000 > vitality 8500 -> effective 0.
        set_context_at(
            ALKEMIS_ID,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
    }

    /// (b) role gate: Extractor tidak boleh transmute.
    #[test]
    #[should_panic(expected = "ERR_ROLE_NOT_ALLOWED")]
    fn vc004_role_gate_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
    }

    /// (c) extract mint 1 spore sesuai jenis.
    #[test]
    fn vc004_extract_mint_sesuai_jenis() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 3); // genesis 3 ignis
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(7));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(7));
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&4));
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-cryo".to_string(), U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:spore-cryo".to_string())), Some(&1));
        // baseline treasury = 20% stake inkubasi (2e11) + katalis 7: fee 7 masuk zona
        // Hypothermia (temp=0); inflow DK-033 = 7×5000/10000 = 3, treasury = 7−3 = 4;
        // rebate 7×200/10000 = 0.
        assert_eq!(c.treasury, 200_000_000_000 + 4);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 3);
    }

    #[test]
    #[should_panic(expected = "ERR_UNKNOWN_TOKEN")]
    fn vc004_extract_token_tak_dikenal_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:bogus".to_string(), U128(0));
    }

    /// (d) transmute burn -> mint x efficiency_bps (10000 = 1:1; 5000 = 0,5x).
    #[test]
    fn vc004_transmute_formula_efficiency() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 10);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.transmute_spores_to_enzyme(1, 4, U128(0));
        assert_eq!(v.efficiency_bps, 10000);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&6));
        assert_eq!(c.inventories.get(&(1, "mt:enzyme-amalgam".to_string())), Some(&4));
        // ubah efficiency jadi 5000 -> output 0,5x
        let mut vessel = c.strains.get(&1).unwrap().clone();
        vessel.efficiency_bps = 5000;
        let _ = c.strains.insert(1, vessel);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 4, U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:enzyme-amalgam".to_string())), Some(&6));
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&2));
    }

    /// (e) resep enzim: redactase (ignis+lumin) & synthase (cryo).
    #[test]
    fn vc004_transmute_resep_redactase_synthase() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 1);
        grant(&mut c, "mt:spore-lumin", 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:enzyme-redactase".to_string())), Some(&2));
        grant(&mut c, "mt:spore-cryo", 2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:enzyme-synthase".to_string())), Some(&2));
    }

    /// (e) resep tak terpenuhi (jumlah ganjil) -> `ERR_RECIPE_NOT_MET`.
    #[test]
    #[should_panic(expected = "ERR_RECIPE_NOT_MET")]
    fn vc004_transmute_resep_tak_terpenuhi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 10);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 3, U128(0));
    }

    /// (h) stok kurang -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn vc004_stok_spora_kurang_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 4, U128(0));
    }

    /// (e) eliksir: homunculus (2 amalgam + 1 synthase) & homeostasis (1+1) per batch.
    #[test]
    fn vc004_synthesize_resep_eliksir() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, SYNTHESIZER_ROLE, 0);
        grant(&mut c, "mt:enzyme-amalgam", 4);
        grant(&mut c, "mt:enzyme-synthase", 2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.synthesize_elixir(1, "mt:elixir-homunculus".to_string(), 6, U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:elixir-homunculus".to_string())), Some(&2));
        grant(&mut c, "mt:enzyme-amalgam", 2);
        grant(&mut c, "mt:enzyme-redactase", 2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.synthesize_elixir(1, "mt:elixir-homeostasis".to_string(), 4, U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:elixir-homeostasis".to_string())), Some(&2));
    }

    /// (f) aether butuh TEE -> `ERR_REQUIRES_TEE_ATTESTATION`.
    #[test]
    #[should_panic(expected = "ERR_REQUIRES_TEE_ATTESTATION")]
    fn vc004_aether_ditolak_tanpa_tee() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, SYNTHESIZER_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.synthesize_elixir(1, "mt:elixir-aether".to_string(), 3, U128(0));
    }

    /// (g) pemulihan metabolik: vitality +500 & last_fed reset.
    #[test]
    fn vc004_pemulihan_metabolik() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
        // effective 8500 (1 jam belum menurunkan 1 penuh), +500 -> 9000.
        assert_eq!(v.vitality, 9000);
        assert_eq!(v.last_fed_sec, 3600);
    }

    /// katalis: deposit != nominal -> `ERR_STAKE_DEPOSIT_MISMATCH`.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn vc004_katalis_mismatch_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(5));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(9));
    }

    /// `migrate()` — pola migrasi state NEAR; dipanggil sekali setelah redeploy.
    #[test]
    fn migrate_membangun_state_baru() {
        let c = SnaccContract::migrate();
        assert_eq!(c.protocol_version(), "snacc-v001");
        assert_eq!(c.ft_total_supply(), U128(0));
    }

    // ----------------------------------------------------------------------
    // VC-005 — F4: Termostat Makro CrucibleTemp (ADR-04, I-01, I-04)
    // ----------------------------------------------------------------------

    /// (1 & 2) Rumus suhu `get_crucible_temp` termasuk lazy decay pada 2 titik waktu (`ADR-04`).
    #[test]
    fn vc005_crucible_temp_lazy_decay_dua_titik_waktu() {
        let mut c = SnaccContract::new();
        // stake = 10_000 -> total_snacc_supply = 10_000
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(c.macro_state.total_snacc_supply, 10_000);

        // Titik waktu 1: t1 = 3600s (3_600_000_000_000 ns), fee = 10_000 -> volume_epoch = 10_000
        // temp(t1) = floor(1000 * 10_000 / 10_000) = 1000 mK (zona netral, supply tetap 10_000)
        let t1_ns = 3_600_000_000_000u64;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(10_000), t1_ns);
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.get_crucible_temp(), 1000);

        // Titik waktu 2: t2 = t1 + 40 detik -> decay = 40 * 50 = 2_000 -> V_decayed = 8_000
        // temp(t2) = floor(1000 * 8_000 / 10_000) = 800 mK
        let t2_ns = t1_ns + 40 * 1_000_000_000;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(0), t2_ns);
        assert_eq!(c.get_crucible_temp(), 800);

        // Setelah 500 detik (decay 25_000 > 10_000) -> saturasi ke 0
        let t3_ns = t1_ns + 500 * 1_000_000_000;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(0), t3_ns);
        assert_eq!(c.get_crucible_temp(), 0);
    }

    /// (3) Calcination Fever (`temp > 1400`): membakar 350 bps + invarian `I-01`
    /// (`total_snacc_supply` turun sebesar `burn`, `total_snacc_burned` naik sama besar, sisa ke treasury).
    #[test]
    fn vc005_fever_burn_350_bps_dan_invarian_i01() {
        let mut c = SnaccContract::new();
        let stake = 10_000u128; // treasury awal = 2_000, strain = 8_000, supply = 10_000
        let _ = call_incubate(&mut c, stake, EXTRACTOR_ROLE, 0);
        let supply_before = c.macro_state.total_snacc_supply;
        let treasury_before = c.treasury;

        // Fee = 20_000 -> volume_epoch = 20_000 -> temp = floor(1000 * 20_000 / 10_000) = 2000 > 1400 (Fever!)
        // burn = 20_000 * 350 / 10_000 = 700; sisa ke treasury = 19_300.
        let fee = 20_000u128;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(fee));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(fee));

        let expected_burn = 700u128;
        assert_eq!(c.macro_state.total_snacc_burned, expected_burn);
        assert_eq!(c.macro_state.total_snacc_supply, supply_before - expected_burn);
        assert_eq!(
            c.macro_state.total_snacc_supply + c.macro_state.total_snacc_burned,
            supply_before
        );
        // `DK-033`: sisa non-burn 19_300 dibagi 50/50 — inflow 9_650, treasury 9_650.
        assert_eq!(c.treasury, treasury_before + 9_650);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 9_650);
    }

    /// (3) Hypothermia (`temp < 600`): rebate 200 bps dari treasury ke `snacc_balances[strain_id]`.
    #[test]
    fn vc005_hypothermia_rebate_200_bps() {
        let mut c = SnaccContract::new();
        let stake = 1_000_000u128; // supply = 1_000_000, treasury = 200_000, strain = 800_000
        let _ = call_incubate(&mut c, stake, EXTRACTOR_ROLE, 0);
        c.advance_epoch(); // snapshot epoch_treasury_initial = 200_000 (kuota 500 bps = 10_000)

        let bal_before = c.snacc_balances.get(&1).copied().unwrap();
        let treasury_before = c.treasury;

        // Fee = 10_000 -> temp = floor(1000 * 10_000 / 1_000_000) = 10 < 600 (Hypothermia)
        // rebate = 10_000 * 200 / 10_000 = 200 (dari treasury ke snacc_balances[1]).
        let fee = 10_000u128;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(fee));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(fee));

        let expected_rebate = 200u128;
        assert_eq!(
            c.snacc_balances.get(&1).copied().unwrap(),
            bal_before + expected_rebate
        );
        // `DK-033`: fee 10_000 dibagi 50/50 (inflow 5_000, treasury +5_000), lalu
        // rebate 200 keluar dari treasury.
        assert_eq!(c.treasury, treasury_before + fee - expected_rebate - 5_000);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 5_000);
        assert_eq!(c.macro_state.epoch_treasury_spent, expected_rebate);
    }

    /// (3) Invarian `I-04`: rebate berhenti saat kuota 500 bps epoch habis (tanpa panic).
    #[test]
    fn vc005_invarian_i04_kuota_epoch_500_bps_habis_tanpa_panic() {
        let mut c = SnaccContract::new();
        // stake = 1_000_000 -> treasury = 200_000 -> max_epoch_drain (500 bps) = 10_000
        let _ = call_incubate(&mut c, 1_000_000u128, EXTRACTOR_ROLE, 0);
        c.advance_epoch();

        // Set sudah terpakai 9_900 dari kuota 10_000 -> sisa kuota = 100.
        c.macro_state.epoch_treasury_spent = 9_900;
        let bal_before = c.snacc_balances.get(&1).copied().unwrap();

        // Fee = 10_000 -> raw_rebate = 200, tapi sisa kuota hanya 100 -> rebate dipotong jadi 100.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.epoch_treasury_spent, 10_000);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), bal_before + 100);

        // Panggilan berikutnya saat kuota habis (spent == 10_000): rebate = 0, tidak panic!
        let bal_after_cap = c.snacc_balances.get(&1).copied().unwrap();
        let treasury_after_cap = c.treasury;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.epoch_treasury_spent, 10_000);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), bal_after_cap);
        // `DK-033`: fee 10_000 dibagi 50/50 — treasury hanya +5_000 (rebate 0, kuota habis).
        assert_eq!(c.treasury, treasury_after_cap + 5_000);
    }

    /// (4) `advance_epoch` mereset kuota `epoch_treasury_spent` dan menyalin `epoch_treasury_initial`.
    #[test]
    fn vc005_advance_epoch_reset_kuota() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000u128, EXTRACTOR_ROLE, 0);
        c.macro_state.epoch_treasury_spent = 10_000;

        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.macro_state.epoch_treasury_initial, c.treasury);

        // Setelah advance_epoch, rebate kembali aktif di epoch baru.
        let bal_before = c.snacc_balances.get(&1).copied().unwrap();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), bal_before + 200);
        assert_eq!(c.macro_state.epoch_treasury_spent, 200);
    }

    /// (3) Zona tengah `600..=1400` netral: tanpa burn dan tanpa rebate.
    #[test]
    fn vc005_zona_tengah_netral() {
        let mut c = SnaccContract::new();
        // stake = 10_000 -> supply = 10_000, treasury = 2_000, strain = 8_000
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);

        // Batas bawah zona tengah: fee = 6_000 -> temp = floor(1000 * 6_000 / 10_000) = 600 mK
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(6_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(6_000));
        assert_eq!(c.get_crucible_temp(), 600);
        assert_eq!(c.macro_state.total_snacc_burned, 0);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000);
        // `DK-033`: fee 6_000 dibagi 50/50 — treasury 2_000+3_000, inflow 3_000.
        assert_eq!(c.treasury, 5_000);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 3_000);

        // Batas atas zona tengah: tambah fee = 8_000 -> volume_epoch = 14_000 -> temp = 1400 mK
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(8_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(8_000));
        assert_eq!(c.get_crucible_temp(), 1400);
        assert_eq!(c.macro_state.total_snacc_burned, 0);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000);
        assert_eq!(c.treasury, 9_000); // +4_000 (50% dari 8_000)
        assert_eq!(c.macro_state.epoch_dividend_inflow, 7_000); // 3_000+4_000
    }

    /// (2) `get_macro_state` konsisten dengan `get_crucible_temp` & serialisasi JSON/Borsh.
    #[test]
    fn vc005_get_macro_state_dan_get_crucible_temp_konsisten() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(9_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(9_000));

        let temp = c.get_crucible_temp();
        let view = c.get_macro_state();
        assert_eq!(temp, 900);
        assert_eq!(view.crucible_temp, temp);
        assert_eq!(view.total_snacc_supply, 10_000);
        assert_eq!(view.volume_epoch, 9_000);

        let json = serde_json::to_string(&view).unwrap();
        let back: state::MacroStateView = serde_json::from_str(&json).unwrap();
        assert_eq!(view, back);
    }

    /// (5) `ft_burn` membakar saldo `snacc_balances[strain_id]` dan menjaga invarian `I-01`.
    #[test]
    fn vc005_ft_burn_bakar_saldo_strain() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain=8_000, supply=10_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.ft_burn(1, U128(3_000));

        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 5_000);
        assert_eq!(c.macro_state.total_snacc_supply, 7_000);
        assert_eq!(c.macro_state.total_snacc_burned, 3_000);
        assert_eq!(c.get_strain(1).state_nonce, 1);
    }

    /// (5) `ft_burn` melebihi saldo strain -> ditolak `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn vc005_ft_burn_saldo_kurang_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain=8_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.ft_burn(1, U128(9_000));
    }

    // ------------------------------------------------------------------
    // VC-006 — F3: TEE zero-trust 2-fase + guardrail I-03
    // ------------------------------------------------------------------

    // Vektor kriptografis RIAL dari NEAR AI Cloud TEE (spike `S1.8`/`S2.6`,
    // `chat_id: c895b8f389fe45f3b369417a6de52536`) — diverifikasi `env::ed25519_verify`.
    const TEE_REQ_SHA: &str =
        "d06094a574a9e212b6756e3fb67457ec3a6fff9a4ff83fb3f48935e852569bb1";
    const TEE_RES_SHA: &str =
        "9014e73a4ed9d1c2234b4d04c4d0897330a9c2b52b10e60fb5b2e3d07f91a21c";
    const TEE_PUBKEY_HEX: &str =
        "5a0210f2442c79355784f9de021985429561da5bacc3a4424b01ac52b7e0caff";
    const TEE_SIG_HEX: &str = "23f722d80dc1eb3b74a9523d57bb8e168700bc6fe6a22e97ad58254b3e353e0d05c04e4af4fa479a769cd18e0d5e44034a94c71370507a1c7f3fc75e2ce1c20d";

    fn envelope(strain_id: u64, shift: i64, vitality_bonus: u64) -> String {
        let content = format!(
            r#"{{"strain_id":{},"efficiency_shift_bps":{},"vitality_bonus":{},"phenotype_trait":"Thermo-Aureate"}}"#,
            strain_id, shift, vitality_bonus
        );
        format!(
            r#"{{"id":"chat-abc","choices":[{{"message":{{"content":"{}"}}}}]}}"#,
            content.replace('"', "\\\"")
        )
    }

    /// (a) verifikasi signature RIAL (`env::ed25519_verify`) atas signed_text spike.
    #[test]
    fn vc006_signature_rial_vector_lulus() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        assert!(SnaccContract::verify_tee_receipt(
            TEE_REQ_SHA,
            TEE_RES_SHA,
            TEE_SIG_HEX,
            TEE_PUBKEY_HEX
        ));
    }

    /// (a2) red-team `S1.9`: ubah 1 byte signature / hash -> verifikasi gagal.
    #[test]
    fn vc006_tamper_satu_byte_ditolak() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let mut sig = TEE_SIG_HEX.to_string();
        sig.replace_range(0..1, "0");
        assert!(!SnaccContract::verify_tee_receipt(
            TEE_REQ_SHA,
            TEE_RES_SHA,
            &sig,
            TEE_PUBKEY_HEX
        ));
        assert!(!SnaccContract::verify_tee_receipt(
            TEE_REQ_SHA,
            "0000000000000000000000000000000000000000000000000000000000000000",
            TEE_SIG_HEX,
            TEE_PUBKEY_HEX
        ));
    }

    /// (b) commit binding: `req_sha256` berbeda dari komitmen -> `ERR_COMMITMENT_MISMATCH`.
    #[test]
    #[should_panic(expected = "ERR_COMMITMENT_MISMATCH")]
    fn vc006_komitmen_tidak_cocok_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.commit_genome_experiment(1, TEE_REQ_SHA.to_string());
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.apply_tee_mutation_zero_trust(
            1,
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            "{}".to_string(),
            TEE_SIG_HEX.to_string(),
            TEE_PUBKEY_HEX.to_string(),
        );
    }

    /// (c) `I-16`: kunci tidak terdaftar -> `ERR_SIGNER_NOT_REGISTERED`.
    #[test]
    #[should_panic(expected = "ERR_SIGNER_NOT_REGISTERED")]
    fn vc006_signer_tidak_terdaftar_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.commit_genome_experiment(1, TEE_REQ_SHA.to_string());
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.apply_tee_mutation_zero_trust(
            1,
            TEE_REQ_SHA.to_string(),
            "{}".to_string(),
            TEE_SIG_HEX.to_string(),
            TEE_PUBKEY_HEX.to_string(),
        );
    }

    /// (d) guardrail `I-03`: `efficiency_shift_bps` di luar ±1500 -> ditolak.
    #[test]
    #[should_panic(expected = "ERR_GUARDRAIL_SHIFT")]
    fn vc006_guardrail_shift_lebih_1500_ditolak() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = SnaccContract::parse_mutation_payload(&envelope(1, 1501, 0), 1);
    }

    /// (d2) guardrail `I-03`: `vitality_bonus > 500` -> ditolak.
    #[test]
    #[should_panic(expected = "ERR_GUARDRAIL_VITALITY")]
    fn vc006_guardrail_vitality_lebih_500_ditolak() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = SnaccContract::parse_mutation_payload(&envelope(1, 0, 501), 1);
    }

    /// (e) payload untuk strain lain -> `ERR_PAYLOAD_STRAIN_MISMATCH`.
    #[test]
    #[should_panic(expected = "ERR_PAYLOAD_STRAIN_MISMATCH")]
    fn vc006_payload_strain_lain_ditolak() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = SnaccContract::parse_mutation_payload(&envelope(2, 450, 200), 1);
    }

    /// (f) parse payload sah (vektor spike: +450 bps, +200 vitality, trait).
    #[test]
    fn vc006_parse_payload_sesuai_vektor_spike() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let p = SnaccContract::parse_mutation_payload(&envelope(1, 450, 200), 1);
        assert_eq!(p.efficiency_shift_bps, 450);
        assert_eq!(p.vitality_bonus, 200);
        assert_eq!(p.phenotype_trait.as_deref(), Some("Thermo-Aureate"));
    }

    /// (g) registrasi signer: bootstrap owner pertama + non-owner ditolak.
    #[test]
    #[should_panic(expected = "ERR_NOT_CONTRACT_OWNER")]
    fn vc006_register_signer_hanya_owner() {
        let mut c = SnaccContract::new();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.register_attested_signer(TEE_PUBKEY_HEX.to_string(), 1_700_000_000);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.register_attested_signer(
            "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            1_700_000_000,
        );
    }

    /// (h) body palsu dengan signature rial -> `ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH`
    /// (replikasi red-team `S1.9`: tidak ada jalan lolos tanpa body yang benar-benar ditandatangani).
    #[test]
    #[should_panic(expected = "ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH")]
    fn vc006_apply_body_palsu_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.register_attested_signer(TEE_PUBKEY_HEX.to_string(), 1_700_000_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.commit_genome_experiment(1, TEE_REQ_SHA.to_string());
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.apply_tee_mutation_zero_trust(
            1,
            TEE_REQ_SHA.to_string(),
            envelope(1, 450, 200),
            TEE_SIG_HEX.to_string(),
            TEE_PUBKEY_HEX.to_string(),
        );
    }

    // ------------------------------------------------------------------
    // VC-007 — F5: splice_strains (Voluntary Coasean Merger, ADR-25)
    // ------------------------------------------------------------------

    fn vc007_pair(c: &mut SnaccContract) {
        // strain 1 (Extractor) & strain 2 (Fermenter), stake 10_000 -> saldo 8_000 tiap.
        let _ = call_incubate(c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(c, 10_000u128, FERMENTER_ROLE, 0);
    }

    /// (a) gabung aset: 97,5% $SNACC + 100% 9 molekul + pajak 2,5% ke treasury.
    #[test]
    fn vc007_splice_gabung_aset_dan_pajak() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c); // saldo 8_000 tiap; treasury 4_000 (20% x2)
        let _ = c.inventories.insert((2, "mt:spore-ignis".to_string()), 5);
        let _ = c.inventories.insert((2, "mt:enzyme-amalgam".to_string()), 3);
        let _ = c.inventories.insert((2, "mt:elixir-aether".to_string()), 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 2);
        // pajak = 8000 * 250 / 10000 = 200; sisa 7800 -> dominant.
        assert_eq!(c.snacc_balances.get(&1), Some(&15_800));
        assert_eq!(c.snacc_balances.get(&2), Some(&0));
        assert_eq!(c.treasury, 4_200);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&5));
        assert_eq!(c.inventories.get(&(1, "mt:enzyme-amalgam".to_string())), Some(&3));
        assert_eq!(c.inventories.get(&(1, "mt:elixir-aether".to_string())), Some(&1));
        assert_eq!(c.inventories.get(&(2, "mt:spore-ignis".to_string())), Some(&0));
    }

    /// (b) bitmask OR (`I-08`), `Gen+1`, `efficiency_bps +800`.
    #[test]
    fn vc007_bitmask_gen_efficiency() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c); // role 0b001 & 0b010
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.splice_strains(1, 2);
        assert_eq!(v.role_mask, 0b011);
        assert_eq!(v.generation, 1);
        assert_eq!(v.efficiency_bps, 10_800); // 10000 + 800
    }

    /// (c) `absorbed` -> `SplicedBurned`; splice ulang ditolak.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_ALREADY_SPLICED")]
    fn vc007_absorbed_burned_dan_tolak_ulang() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 2);
        assert_eq!(c.get_strain(2).status, StrainStatus::SplicedBurned);
        let _ = c.splice_strains(1, 2);
    }

    /// (d) pemilik berbeda -> `ERR_NOT_SAME_OWNER`.
    #[test]
    #[should_panic(expected = "ERR_NOT_SAME_OWNER")]
    fn vc007_beda_pemilik_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(10_000), 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 2);
    }

    /// (e) id sama -> ditolak.
    #[test]
    #[should_panic(expected = "ERR_SAME_STRAIN")]
    fn vc007_id_sama_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 1);
    }

    /// `SEC-101`: `ft_burn` oleh non-owner ditolak (temuan `free-pi-cli`: tanpa cek
    /// pemanggil, siapa pun bisa membakar saldo Strain orang lain).
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn sec101_ft_burn_non_owner_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.ft_burn(1, U128(1_000));
    }

    // ------------------------------------------------------------------
    // VC-008 — F6: harvest_starving_strain (Cell Lysis, ADR-11 / I-10)
    // ------------------------------------------------------------------

    /// (a) split 90/10, permissionless, kas + inventori dibersihkan, status Liquidated.
    #[test]
    fn vc008_harvest_split_90_10() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // saldo 8_000; treasury 2_000
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 5);
        // 200 jam x BMR 100 = 20000 > 8500 -> effective_vitality = 0.
        set_context_at(
            "raider.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let (bounty, recovery) = c.harvest_starving_strain(1);
        assert_eq!(bounty.0, 800); // 10% dari 8_000
        assert_eq!(recovery.0, 7_200); // 90%
        assert_eq!(c.account_balances.get(&"raider.testnet".parse::<AccountId>().unwrap()), Some(&800));
        assert_eq!(c.treasury, 9_200); // 2_000 + 7_200
        assert_eq!(c.snacc_balances.get(&1), Some(&0));
        assert_eq!(c.get_strain(1).status, StrainStatus::Liquidated);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&0));
    }

    /// (b) `I-10`: target masih hidup -> `ERR_STRAIN_NOT_INSOLVENT`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_INSOLVENT")]
    fn vc008_target_masih_hidup_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        let _ = c.harvest_starving_strain(1);
    }

    /// (c) double harvest -> `ERR_ALREADY_LIQUIDATED`.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn vc008_double_harvest_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context_at(
            "raider.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        let _ = c.harvest_starving_strain(1);
    }

    // ------------------------------------------------------------------
    // VC-009 — F7: nft_token — Dynamic Base64 SVG (NEP-177, ADR-03)
    // ------------------------------------------------------------------

    /// (a) media = data URI base64, deterministik, payload wajar.
    #[test]
    fn vc009_nft_token_media_dan_deterministik() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let t1 = c.nft_token("1".to_string());
        let t2 = c.nft_token("1".to_string());
        assert_eq!(t1, t2);
        assert!(t1.metadata.media.starts_with("data:image/svg+xml;base64,"));
        assert_eq!(t1.token_id, "1");
        assert_eq!(t1.owner_id.to_string(), ALKEMIS_ID);
        assert!(t1.metadata.title.contains("Snacc Strain #1"));
        // SVG ≤ 1.734 byte (diuji unit di svg.rs) -> base64 payload ≤ ±2.340 karakter.
        assert!(t1.metadata.media.len() < 2_400, "media {} byte", t1.metadata.media.len());
    }

    /// (b) token id tidak valid / tidak ada -> `ERR_STRAIN_NOT_FOUND`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn vc009_nft_token_tidak_ada_ditolak() {
        let c = SnaccContract::new();
        let _ = c.nft_token("bukan-angka".to_string());
    }

    /// (c) strain terlikuidasi tetap bisa dirender (merah) via `nft_token`.
    #[test]
    fn vc009_nft_token_setelah_likuidasi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context_at(
            "raider.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        let t = c.nft_token("1".to_string());
        assert!(t.metadata.media.starts_with("data:image/svg+xml;base64,"));
    }

    // ------------------------------------------------------------------
    // VC-010 — event NEP-297 ber-standard "snacc" (EVENT_JSON:)
    // ------------------------------------------------------------------

    /// (a) tiap aksi state memancarkan `EVENT_JSON` dengan `standard:"snacc"`.
    /// Catatan: `testing_env!` meng-reset log, jadi log dicek per aksi.
    #[test]
    fn vc010_event_dipancarkan_per_aksi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 3); // -> strain_created
        let l1 = near_sdk::test_utils::get_logs().join("\n");
        assert!(l1.contains(r#""event":"strain_created""#), "strain_created hilang");
        assert!(l1.contains(r#""standard":"snacc""#));

        set_context(ALKEMIS_ID, NearToken::from_yoctonear(5));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(5)); // -> extracted
        let l2 = near_sdk::test_utils::get_logs().join("\n");
        assert!(l2.contains(r#""event":"extracted""#), "extracted hilang");

        set_context_at(
            "raider.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1); // -> strain_liquidated
        let l3 = near_sdk::test_utils::get_logs().join("\n");
        assert!(l3.contains(r#""event":"strain_liquidated""#), "strain_liquidated hilang");
    }

    // ------------------------------------------------------------------
    // N141-101..106 — surface standar NEP-141 ($SNACC)
    // ------------------------------------------------------------------

    const ALICE: &str = "alice.testnet";
    const BOB: &str = "bob.testnet";

    /// Daftarkan akun (storage deposit MVP 5 milliNEAR).
    fn register(c: &mut SnaccContract, account: &str) {
        set_context(account, NearToken::from_millinear(constants::STORAGE_DEPOSIT_MILLINEAR));
        let _ = c.storage_deposit(None);
    }

    fn fund(c: &mut SnaccContract, account: &str, amount: u128) {
        let _ = c
            .account_balances
            .insert(account.parse::<AccountId>().unwrap(), amount);
    }

    fn n141_setup() -> (SnaccContract, u128) {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain bal 8_000
        register(&mut c, ALKEMIS_ID);
        register(&mut c, ALICE);
        register(&mut c, BOB);
        let supply = c.macro_state.total_snacc_supply;
        (c, supply)
    }

    /// (1) transfer sukses + event `NEP-297` ber-standard `nep141`.
    #[test]
    fn n141_ft_transfer_sukses_dan_event() {
        let (mut c, supply) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(ALICE.parse().unwrap(), U128(400), Some("lab fee".to_string()));
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 600);
        assert_eq!(c.account_balance(&ALICE.parse().unwrap()), 400);
        // Transfer tidak mengubah suplai (`I-01`).
        assert_eq!(c.macro_state.total_snacc_supply, supply);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""standard":"nep141""#), "standard nep141 hilang");
        assert!(logs.contains(r#""event":"ft_transfer""#), "event ft_transfer hilang");
        assert!(logs.contains(r#""old_owner_id":"alkemis.testnet""#));
        assert!(logs.contains(r#""new_owner_id":"alice.testnet""#));
    }

    /// (2) penerima tak terdaftar -> `ERR_ACCOUNT_NOT_REGISTERED`.
    #[test]
    #[should_panic(expected = "ERR_ACCOUNT_NOT_REGISTERED")]
    fn n141_transfer_ke_tak_terdaftar_ditolak() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer("raider.testnet".parse().unwrap(), U128(100), None);
    }

    /// (3) saldo kurang -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn n141_saldo_kurang_ditolak() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 100);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(ALICE.parse().unwrap(), U128(400), None);
    }

    /// (4) tanpa 1 yoctoNEAR -> `ERR_YOCTO_REQUIRED` (keputusan tercatat: spec tidak menamai).
    #[test]
    #[should_panic(expected = "ERR_YOCTO_REQUIRED")]
    fn n141_tanpa_satu_yocto_ditolak() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.ft_transfer(ALICE.parse().unwrap(), U128(100), None);
    }

    /// (5) self-transfer netral: saldo & total supply tidak berubah.
    #[test]
    fn n141_self_transfer_netral() {
        let (mut c, supply) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(ALKEMIS_ID.parse().unwrap(), U128(400), None);
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 1_000);
        assert_eq!(c.macro_state.total_snacc_supply, supply);
    }

    /// (6a) `ft_transfer_call`: jalur callback gagal -> refund penuh ke pengirim.
    #[test]
    fn n141_transfer_call_refund_penuh_saat_gagal() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.ft_transfer_call(ALICE.parse().unwrap(), U128(400), None, "x".to_string());
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 600);
        assert_eq!(c.account_balance(&ALICE.parse().unwrap()), 400);

        // Simulasikan callback dengan promise gagal (`ft_on_transfer` tidak merespons).
        let mut builder = VMContextBuilder::new();
        builder
            .predecessor_account_id("snacc-v001.near".parse().unwrap())
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp(3_600_000_000_000);
        testing_env!(
            builder.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![near_sdk::PromiseResult::Failed]
        );
        let refunded = c.on_transfer_call(
            ALKEMIS_ID.parse().unwrap(),
            ALICE.parse().unwrap(),
            U128(400),
        );
        assert_eq!(refunded.0, 400);
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 1_000);
        assert_eq!(c.account_balance(&ALICE.parse().unwrap()), 0);
    }

    /// (6b) `ft_transfer_call`: `ft_on_transfer` sukses pakai sebagian -> refund sisanya.
    #[test]
    fn n141_transfer_call_refund_unused() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.ft_transfer_call(ALICE.parse().unwrap(), U128(400), None, "x".to_string());

        let mut builder = VMContextBuilder::new();
        builder
            .predecessor_account_id("snacc-v001.near".parse().unwrap())
            .attached_deposit(NearToken::from_yoctonear(0))
            .block_timestamp(3_600_000_000_000);
        testing_env!(
            builder.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![near_sdk::PromiseResult::Successful(
                near_sdk::serde_json::to_vec(&U128(150)).unwrap()
            )]
        );
        let refunded = c.on_transfer_call(
            ALKEMIS_ID.parse().unwrap(),
            ALICE.parse().unwrap(),
            U128(400),
        );
        assert_eq!(refunded.0, 150);
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 750);
        assert_eq!(c.account_balance(&ALICE.parse().unwrap()), 250);
    }

    /// (7) `ft_metadata` benar (decimals 24 = keputusan tercatat).
    #[test]
    fn n141_ft_metadata_benar() {
        let c = SnaccContract::new();
        let m = c.ft_metadata();
        assert_eq!(m.spec, "ft-1.0.0");
        assert_eq!(m.name, "Snacc");
        assert_eq!(m.symbol, "$SNACC");
        assert_eq!(m.decimals, 24);
        assert!(m.icon.unwrap().starts_with("data:image/svg+xml;base64,"));
        assert_eq!(m.reference, None);
        assert_eq!(m.reference_hash, None);
    }

    /// (8) `ft_total_supply` konsisten `I-01` (bertambah saat inkubasi, turun saat burn).
    #[test]
    fn n141_ft_total_supply_konsisten_i01() {
        let (mut c, _) = n141_setup();
        assert_eq!(c.ft_total_supply().0, 10_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.ft_burn(1, U128(3_000));
        assert_eq!(c.ft_total_supply().0, 7_000);
    }

    /// (9) storage deposit / withdraw / balance_of (MVP).
    #[test]
    fn n141_storage_siklus_lengkap() {
        let mut c = SnaccContract::new();
        register(&mut c, ALICE);
        let alice: AccountId = ALICE.parse().unwrap();
        let bal = c.storage_balance_of(alice.clone()).unwrap();
        assert_eq!(bal.total.0, NearToken::from_millinear(5).as_yoctonear());

        // Tambatkan deposit lagi -> total bertambah.
        set_context(ALICE, NearToken::from_millinear(3));
        let bal2 = c.storage_deposit(Some(alice.clone()));
        assert_eq!(bal2.total.0, NearToken::from_millinear(8).as_yoctonear());

        // Belum memegang $SNACC -> bisa tarik penuh; entri terhapus (batal registrasi).
        set_context(ALICE, NearToken::from_yoctonear(0));
        let bal3 = c.storage_withdraw(None);
        assert_eq!(bal3.total.0, 0);
        assert_eq!(c.storage_balance_of(alice.clone()), None);

        // Memegang $SNACC -> minimum 5 milliNEAR terkunci.
        register(&mut c, ALICE);
        fund(&mut c, ALICE, 500);
        set_context(ALICE, NearToken::from_yoctonear(0));
        let bal4 = c.storage_withdraw(None);
        assert_eq!(bal4.total.0, NearToken::from_millinear(5).as_yoctonear());
        assert_eq!(bal4.available.0, 0);
        assert!(c.storage_balance_of(alice).is_some());
    }

    /// (10) `get_total_wealth` = account_balances + Σ saldo strain miliknya.
    #[test]
    fn n141_get_total_wealth() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 100); // strain 1 milik ALKEMIS, saldo 8_000
        assert_eq!(c.get_total_wealth(ALKEMIS_ID.parse().unwrap()).0, 8_100);
        assert_eq!(c.ft_balance_of(ALKEMIS_ID.parse().unwrap()).0, 100);
        assert_eq!(c.ft_balance_of(ALICE.parse().unwrap()).0, 0);
    }

    /// (11) deposit storage di bawah minimum -> `ERR_INSUFFICIENT_DEPOSIT`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn n141_storage_deposit_kurang_ditolak() {
        let mut c = SnaccContract::new();
        set_context(ALICE, NearToken::from_millinear(4));
        let _ = c.storage_deposit(None);
    }

    // ------------------------------------------------------------------
    // DK-033 / DK-044 — Industrial Dividend Pool (ADR-28)
    // ------------------------------------------------------------------

    /// Setup dividen: strain 1 = Extractor (pembayar fee katalis), strain 2 =
    /// Synthesizer (kontributor/klaim dividen). Supply 20_000, treasury 4_000.
    fn dk_setup() -> SnaccContract {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, SYNTHESIZER_ROLE, 0);
        c
    }

    /// Skema dasar: fee katalis → inflow; bakar aether → kontribusi; tutup epoch →
    /// rate; klaim pro-rata tepat.
    #[test]
    fn dk033_skema_dividen_dasar_pro_rata() {
        let mut c = dk_setup();
        // Epoch 0: fee 10_000 (temp = 1000, zona netral) -> inflow 5_000.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.epoch_dividend_inflow, 5_000);

        // Bakar 2 aether sebagai kontribusi (strain 2 = Synthesizer).
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 5);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(2));
        assert_eq!(c.macro_state.epoch_aether_burned_total, 2);
        assert_eq!(c.inventories.get(&(2, constants::ELIXIR_AETHER.to_string())), Some(&3));

        // Tutup epoch: rate = 5_000 × 1e18 / 2 = 2_500 × 1e18; pool = 5_000.
        c.advance_epoch();
        assert_eq!(c.get_epoch_dividend_rate(0).0, 2_500_000_000_000_000_000_000); // 2_500 × 1e18
        assert_eq!(c.get_dividend_pool(), (U128(5_000), U128(0), 1));

        // Klaim: payout = 2 × 2_500e18 / 1e18 = 5_000 (pro-rata penuh, satu-satunya kontributor).
        let bal_before = c.snacc_balances.get(&2).copied().unwrap();
        let payout = c.claim_industrial_dividend(2, 0);
        assert_eq!(payout.0, 5_000);
        assert_eq!(c.snacc_balances.get(&2).copied().unwrap(), bal_before + 5_000);
        assert_eq!(c.get_dividend_pool().0 .0, 0);
        let (contrib, claimed) = c.get_dividend_contribution(0, 2);
        assert_eq!(contrib.0, 2);
        assert!(claimed);
        // `TC-DIV-10`: nonce naik saat payout (`ADR-29`).
        assert!(c.get_strain(2).state_nonce > 0);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"dividend_claimed""#));
    }

    /// `TC-DIV-03` — klaim ganda ditolak.
    #[test]
    #[should_panic(expected = "ERR_DIVIDEND_ALREADY_CLAIMED")]
    fn dk033_klaim_ganda_ditolak() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.advance_epoch();
        let _ = c.claim_industrial_dividend(2, 0);
        let _ = c.claim_industrial_dividend(2, 0);
    }

    /// `TC-DIV-04` — klaim epoch berjalan -> `ERR_EPOCH_NOT_CLOSED`.
    #[test]
    #[should_panic(expected = "ERR_EPOCH_NOT_CLOSED")]
    fn dk033_klaim_epoch_berjalan_ditolak() {
        let mut c = dk_setup();
        let _ = c.claim_industrial_dividend(1, 0);
    }

    /// Batas klaim mundur `MAX_DIVIDEND_CLAIM_EPOCHS = 8` -> `ERR_DIVIDEND_CLAIM_EXPIRED`.
    #[test]
    #[should_panic(expected = "ERR_DIVIDEND_CLAIM_EXPIRED")]
    fn dk033_klaim_kedaluwarsa_ditolak() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        // Tutup 10 epoch (inflow kosong di epoch 1..10).
        for _ in 0..10 {
            set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
            c.advance_epoch();
        }
        let _ = c.claim_industrial_dividend(2, 0);
    }

    /// `TC-DIV-05` — payout di-cap ke saldo kolam; tidak pernah mint/negatif.
    #[test]
    fn dk033_cap_pool_dan_konservasi() {
        let mut c = dk_setup(); // strain 1 Extractor, strain 2 Synthesizer
        // Fee 10_000 dari strain 1 -> inflow 5_000.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        // Strain ketiga = Synthesizer tambahan.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(SYNTHESIZER_ROLE), U128(0), 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        // strain 2 & 3 bakar 1 & 3 aether (total 4).
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 1);
        let _ = c.inventories.insert((3, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.synthesize_and_redeem_aether(3, U128(3));
        c.advance_epoch();
        // rate = 10_000 × 1e18 / 4 = 2_500e18.
        let pool = c.get_dividend_pool().0 .0;
        assert_eq!(pool, 10_000); // inflow 5_000 + 5_000
        let p2 = c.claim_industrial_dividend(2, 0).0;
        let p3 = c.claim_industrial_dividend(3, 0).0;
        assert_eq!(p2, 2_500); // 1 × 2_500e18 / 1e18
        assert_eq!(p3, 7_500); // 3 × 2_500e18 / 1e18
        // `TC-DIV-05`: Σ payout ≤ pool; kolam habis tepat.
        assert!(p2 + p3 <= pool);
        assert_eq!(c.get_dividend_pool().0 .0, 0);
    }

    /// Kolam kosong (rate 0) -> `ERR_DIVIDEND_POOL_EMPTY`.
    #[test]
    #[should_panic(expected = "ERR_DIVIDEND_POOL_EMPTY")]
    fn dk033_kolam_kosong_ditolak() {
        let mut c = dk_setup();
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.advance_epoch(); // tanpa fee -> inflow 0 -> rate 0 -> payout 0
        let _ = c.claim_industrial_dividend(2, 0);
    }

    /// `TC-DIV-06` — tanpa bit `ROLE_SYNTHESIZER` -> `ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE`.
    #[test]
    #[should_panic(expected = "ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE")]
    fn dk044_role_bukan_synthesizer_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = c.inventories.insert((1, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(1, U128(1));
    }

    /// `TC-DIV-07` — akun lain bakar aether strain orang lain -> `ERR_NOT_STRAIN_OWNER`.
    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn dk044_non_owner_ditolak() {
        let mut c = dk_setup();
        let _ = c.inventories.insert((1, constants::ELIXIR_AETHER.to_string()), 3);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(1, U128(1));
    }

    /// Stok aether kurang / amount 0 (< `MIN_AETHER_BURN`) -> `ERR_INSUFFICIENT_AETHER`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_AETHER")]
    fn dk044_aether_kurang_ditolak() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
    }

    /// Strain kelaparan (effective_vitality == 0) -> `ERR_STRAIN_STARVING`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn dk044_strain_kelaparan_ditolak() {
        let mut c = dk_setup();
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context_at(
            ALKEMIS_ID,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        c.synthesize_and_redeem_aether(2, U128(1));
    }

    /// `TC-DIV-02` — anti first-mover: dua strain, urutan klaim dibalik, payout total
    /// identik (rate dikunci saat `advance_epoch`).
    #[test]
    fn dk033_anti_first_mover() {
        let mut c = dk_setup(); // strain 1 Extractor, strain 2 Synthesizer
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(SYNTHESIZER_ROLE), U128(0), 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        let _ = c.inventories.insert((3, constants::ELIXIR_AETHER.to_string()), 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(3, U128(1)); // strain 3 (kontribusi kecil) lebih dulu
        c.synthesize_and_redeem_aether(2, U128(3));
        c.advance_epoch();
        let p3 = c.claim_industrial_dividend(3, 0).0; // urutan klaim dibalik
        let p2 = c.claim_industrial_dividend(2, 0).0;
        // inflow 10_000; kontribusi 3:1 -> payout 7_500 : 2_500 (pro-rata, bukan first-mover).
        assert_eq!(p3, 2_500);
        assert_eq!(p2, 7_500);
        assert_eq!(p2 + p3, 10_000);
    }

    // ------------------------------------------------------------------
    // EN-101..104 — Nutrient Burst (spec Enhancement §3)
    // ------------------------------------------------------------------

    /// `EN-101` — getter `get_burst_state` (0 sebelum ada likuidasi).
    #[test]
    fn en101_get_burst_state_awal_nol() {
        let c = dk_setup();
        assert_eq!(c.get_burst_state(), (U128(0), 0));
    }

    /// `EN-102` — `harvest_starving_strain` menyalakan burst: `min(BASE + scaled, MAX)`,
    /// `until = epoch + 1`, event `nutrient_burst_released`.
    #[test]
    fn en102_harvest_menyalakan_burst() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // saldo 8_000, supply 10_000
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 5);
        set_context_at(
            "raider.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let (_bounty, recovery) = c.harvest_starving_strain(1);
        assert_eq!(recovery.0, 7_200);
        // scaled_share = 7_200 × 10000 / 10_000 = 7_200 -> burst = min(200+7_200, 2_000) = 2_000.
        assert_eq!(c.macro_state.nutrient_burst_bps, 2_000);
        assert_eq!(c.macro_state.nutrient_burst_until, 1); // current_epoch 0 + 1
        assert_eq!(c.get_burst_state(), (U128(2_000), 1));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"nutrient_burst_released""#), "burst event hilang");
        assert!(logs.contains(r#""released_vita":7200"#));
        assert!(logs.contains(r#""until_epoch":1"#));
    }

    /// `TC-NB-02` — likuidasi kedua tidak melampaui `MAX_BURST_BPS`.
    #[test]
    fn en102_tc_nb02_burst_tak_melebihi_cap() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
        let ts = 3_600_000_000_000 + 200 * 3_600_000_000_000;
        set_context_at("raider.testnet", NearToken::from_yoctonear(0), ts);
        let _ = c.harvest_starving_strain(1);
        let _ = c.harvest_starving_strain(2);
        assert!(c.macro_state.nutrient_burst_bps <= constants::MAX_BURST_BPS);
    }

    /// `EN-103` — `feed_strain`: diskon burst mengurangi fee efektif; normal setelah
    /// `until` (`TC-NB-03`); event `fed`. Catatan: fee kecil di supply kecil selalu
    /// ber zona Hypothermia (temp < 600) — debit bersih = effective_fee − rebate.
    #[test]
    fn en103_feed_diskon_burst_dan_kedaluwarsa() {
        let mut c = dk_setup(); // saldo strain 8_000, treasury 4_000
        // Dormansi dinonaktifkan agar test fokus ke diskon burst (dormansi dites terpisah).
        c.macro_state.params.dormancy_threshold_bps = 0;
        // Nyalakan burst manual (pola sama dengan EN-102).
        c.macro_state.nutrient_burst_bps = 500;
        c.macro_state.nutrient_burst_until = 1;
        c.macro_state.epoch_treasury_initial = c.treasury;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.feed_strain(1, U128(1_000));
        // burst 500 -> 950; rescue (eff 8500) mult 8800 -> 950×8800/10000 = 836;
        // hypo: rebate 836×200/10000 = 16 -> debit bersih 820.
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        assert_eq!(bal0 - bal1, 836 - 16);
        assert_eq!(v.last_fed_sec, 3600); // metabolisme pulih
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"fed""#), "event fed hilang");
        assert!(logs.contains(r#""effective_fee":836"#));
        assert!(logs.contains(r#""burst_bps":500"#));
        assert!(logs.contains(r#""rescue_bps":8800"#));

        // `TC-NB-03`: burst kedaluwarsa setelah `until` — burst hilang (rescue tetap).
        c.advance_epoch(); // current_epoch = 1 == until -> burst mati
        assert_eq!(c.get_burst_state(), (U128(0), 1));
        let bal2 = c.snacc_balances.get(&1).copied().unwrap();
        c.feed_strain(1, U128(1_000));
        let bal3 = c.snacc_balances.get(&1).copied().unwrap();
        // Tanpa burst: f1 = 1_000; rescue eff 9000 -> mult 9200 -> 920;
        // volume 836+920 = 1_756 -> temp 87 (hypo); rebate 920×200/10000 = 18.
        assert_eq!(bal2 - bal3, 920 - 18);
    }

    /// `EN-104` — cap diskon 5% Pantry/epoch (kuota `I-04` dipakai bersama rebate).
    #[test]
    fn en104_cap_diskon_5_pantry_per_epoch() {
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0; // dormansi dites terpisah
        c.macro_state.nutrient_burst_bps = 2_000;
        c.macro_state.nutrient_burst_until = 1;
        // Kuota I-04 = 500 bps dari epoch_treasury_initial 10_000 = 500.
        c.macro_state.epoch_treasury_initial = 10_000;
        c.macro_state.epoch_burst_discount_granted = 490;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        // raw discount = 200; sisa kuota 500−0−490 = 10 -> diskon 10; f1 = 990;
        // rescue (eff 8500) mult 8800 -> f2 = 871; hypo: rebate 871×200/10000 = 17.
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        assert_eq!(bal0 - bal1, 871 - 17);
        assert_eq!(c.macro_state.epoch_burst_discount_granted, 500); // cap tercapai
        assert_eq!(c.macro_state.epoch_treasury_spent, 17);
    }

    /// `TC-NB-01` — loop 1.000 feed saat burst aktif: saldo pemain turun bersih
    /// (`DK-030`: diskon tidak boleh membuat feed menguntungkan) + konservasi `I-01`.
    #[test]
    fn en104_tc_nb01_loop_1000_feed_tetap_merugi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000u128, EXTRACTOR_ROLE, 0); // saldo 8e8, treasury 2e8
        c.macro_state.params.dormancy_threshold_bps = 0; // dormansi dites terpisah
        c.macro_state.nutrient_burst_bps = 2_000;
        c.macro_state.nutrient_burst_until = 1;
        c.macro_state.epoch_treasury_initial = c.treasury;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        for i in 0..1_000 {
            if i % 10 == 0 {
                // testing_env! membuat ulang blockchain mock -> buffer log 16 KB ter-reset
                // (storage dipertahankan); loop 1.000 feed memancarkan 2 event/feed.
                set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
            }
            c.feed_strain(1, U128(1_000));
        }
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        // `DK-030` (TC-NB-01): net loss bersih — setiap feed rugi (rebate 200 bps < fee);
        // rescue menurunkan fee saat vitality naik, tapi tidak pernah membuat untung.
        assert!(bal1 < bal0, "feed harus rugi bersih");
        assert!(bal0 - bal1 > 500_000, "net loss substansial");
        // Rebate ≤ fee yang dibakar/dipungut (`DK-030`): kuota subsidy ≤ volume feed.
        assert!(c.macro_state.epoch_treasury_spent <= c.macro_state.epoch_feast_volume);
        // Konservasi `DK-033` (inflow epoch berjalan dihitung terpisah); supply tak berubah
        // karena zona selalu di bawah Fever (volume maks ~8e5 << 1.4e9).
        assert_eq!(
            c.treasury
                + c.macro_state.industrial_dividend_pool
                + c.macro_state.epoch_dividend_inflow
                + c.macro_state.total_snacc_burned
                + bal1,
            1_000_000_000
        );
    }

    /// `EN-103` — feed oleh non-owner ditolak; feed 0 ditolak.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn en103_feed_non_owner_ditolak() {
        let mut c = dk_setup();
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(100));
    }

    /// `EN-103` — feed melebihi saldo strain -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn en103_feed_saldo_kurang_ditolak() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(9_000)); // saldo 8_000
    }

    /// `F2` — aether kini dapat disintesis bila strain pernah sukses mutasi TEE
    /// (`phenotype_trait.is_some()`); tanpa mutasi tetap `ERR_REQUIRES_TEE_ATTESTATION`.
    #[test]
    fn dk044_aether_tersintesis_setelah_mutasi_tee() {
        let mut c = dk_setup();
        let mut v = c.strains.get(&2).unwrap().clone();
        v.phenotype_trait = Some("Thermo-Aureate".to_string());
        let _ = c.strains.insert(2, v);
        // Resep aether (keputusan tercatat): 2 amalgam + 1 synthase per batch.
        let _ = c.inventories.insert((2, "mt:enzyme-amalgam".to_string()), 2);
        let _ = c.inventories.insert((2, "mt:enzyme-synthase".to_string()), 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.synthesize_elixir(2, "mt:elixir-aether".to_string(), 3, U128(0));
        assert_eq!(c.inventories.get(&(2, "mt:elixir-aether".to_string())), Some(&1));
    }

    // ------------------------------------------------------------------
    // TUGAS 2 #1 — hostile_takeover_splice (Chapter 11 Hostile M&A, ADR-26/I-09/I-14)
    // ------------------------------------------------------------------

    const RAIDER: &str = "raider.testnet";

    /// Setup hostile: strain 1 = Extractor ALKEMIS (target), strain 2 = Fermenter
    /// RAIDER (stake 1_000_000 -> saldo 800_000). Target dibuat distress (eff 1500).
    fn hostile_setup() -> SnaccContract {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // target, bal 8_000
        set_context(RAIDER, NearToken::from_yoctonear(1_000_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(1_000_000), 0); // raider
        // Target distress: 70 jam × BMR 100 = 7_000 -> eff 1_500 (Tier 2: 1..=2000).
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        c
    }

    /// (a) takeover sukses: bayar 115% book value, split 95/5, serap aset + role OR.
    #[test]
    fn hostile_sukses_split_95_5_dan_serap() {
        let mut c = hostile_setup();
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 5); // book +5
        // book = 8_000 + 5 = 8_005; payment = 8_005 × 115% = 9_205 (floor);
        // seller 95% = 8_744; treasury 5% = 461.
        let (payment, view) = c.hostile_takeover_splice(2, 1, 0);
        assert_eq!(payment.0, 9_205);
        assert_eq!(c.snacc_balances.get(&2).copied().unwrap(), 800_000 - 9_205 + 8_000);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 0);
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 8_744);
        assert_eq!(c.treasury, 202_000 + 461); // 2_000+2_000 (inkubasi) + 200_000 (stake raider) + 461
        // Serap: role OR (0b010 | 0b001), gen+1, efficiency max, inventori pindah.
        assert_eq!(view.role_mask, 0b011);
        assert_eq!(view.generation, 1);
        assert_eq!(c.get_strain(1).status, StrainStatus::SplicedBurned);
        assert_eq!(c.inventories.get(&(2, "mt:spore-ignis".to_string())), Some(&5));
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&0));
        // `D8` log + event.
        assert_eq!(c.mna_logs.len(), 1);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"hostile_takeover_spliced""#));
        assert!(logs.contains(r#""payment":9205"#));
    }

    /// (b) target sehat (eff 8500 > 2000) -> `ERR_NOT_DISTRESSED` (I-09).
    #[test]
    #[should_panic(expected = "ERR_NOT_DISTRESSED")]
    fn hostile_bukan_distressed_ditolak() {
        let mut c = hostile_setup();
        // Konteks kembali ke ts awal: eff target masih 8500 (belum 70 jam).
        set_context_at(RAIDER, NearToken::from_yoctonear(0), 3_600_000_000_000);
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    /// (c) `I-14`: nonce tidak cocok -> `ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED`.
    #[test]
    #[should_panic(expected = "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED")]
    fn hostile_nonce_mismatch_ditolak() {
        let mut c = hostile_setup();
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.hostile_takeover_splice(2, 1, 99);
    }

    /// (d) `I-14`: callback berjalan -> `ERR_PENDING_ASYNC_CALLBACK_LOCKED`.
    #[test]
    #[should_panic(expected = "ERR_PENDING_ASYNC_CALLBACK_LOCKED")]
    fn hostile_pending_callback_ditolak() {
        let mut c = hostile_setup();
        let mut t = c.strains.get(&1).unwrap().clone();
        t.pending_callbacks = 1;
        let _ = c.strains.insert(1, t);
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    /// (e) pemanggil bukan pemilik raider -> `ERR_NOT_OWNED_BY_CALLEE` (V-01).
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn hostile_raider_bukan_milik_pemanggil() {
        let mut c = hostile_setup();
        set_context("intruder.testnet", NearToken::from_yoctonear(0));
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    /// (f) saldo raider < 115% book value -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn hostile_saldo_raider_kurang() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // target bal 8_000
        set_context(RAIDER, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(10_000), 0); // raider bal 8_000
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        // payment = 9_200 > raider 8_000.
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    /// (g) target sudah Liquidated -> `ERR_STRAIN_ALREADY_SPLICED`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_ALREADY_SPLICED")]
    fn hostile_target_liquidated_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(1_000_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(1_000_000), 0);
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1); // target jadi Liquidated
        set_context(RAIDER, NearToken::from_yoctonear(0));
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    // ------------------------------------------------------------------
    // TUGAS 2 #2 — EN-105..112 (Great Feast, zona, leaderboard, dormansi, rescue, diet, meter)
    // ------------------------------------------------------------------

    /// `EN-105` — band Goldilocks: UNDERFED / GOLDILOCKS / OVERFED terukur.
    #[test]
    fn en105_feast_band_goldilocks() {
        // UNDERFED: tidak ada feed sama sekali.
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        c.advance_epoch();
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"feast_result""#));
        assert!(logs.contains(r#""verdict":"UNDERFED""#));

        // GOLDILOCKS: target = 5% × 20_000 = 1_000; band ±20% = [800, 1_200];
        // satu feed 1_000 -> effective 880 (rescue 8800) di dalam band.
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        c.advance_epoch();
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""verdict":"GOLDILOCKS""#));
        assert!(logs.contains(r#""volume":880"#));
        assert_eq!(c.macro_state.epoch_feast_volume, 0); // ter-reset per epoch

        // OVERFED: 3 feed -> 880+920+960 = 2_760 > 1_200.
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        c.feed_strain(1, U128(1_000));
        c.feed_strain(1, U128(1_000));
        c.advance_epoch();
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""verdict":"OVERFED""#));
    }

    /// `EN-106` — event `zone_changed {from_zone, to_zone, crucible_temp}` saat zona berganti.
    #[test]
    fn en106_zone_changed_event() {
        let mut c = dk_setup(); // supply 20_000
        // Extract fee 10_000 -> volume 10_000 -> temp 500 -> NORMAL->HYPO.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"zone_changed""#));
        assert!(logs.contains(r#""from_zone":"NORMAL""#));
        assert!(logs.contains(r#""to_zone":"HYPO""#));
        assert!(logs.contains(r#""crucible_temp":500"#));
        // Extract lagi -> volume 20_000 -> temp 1000 -> HYPO->NORMAL.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""from_zone":"HYPO""#));
        assert!(logs.contains(r#""to_zone":"NORMAL""#));
        assert!(logs.contains(r#""crucible_temp":1000"#));
        assert_eq!(c.macro_state.last_zone, state::MacroZone::Normal);
    }

    /// `EN-107` — top-3 feeder per epoch, batas 3 entri, urut menurun.
    #[test]
    fn en107_top_feeder_maks_3() {
        let mut c = SnaccContract::new();
        c.macro_state.params.dormancy_threshold_bps = 0;
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 1 ALKEMIS
        for (acct, stake) in [(RAIDER, 10_000u128), ("carol.testnet", 10_000), ("dave.testnet", 10_000)] {
            set_context(acct, NearToken::from_yoctonear(stake));
            let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(stake), 0);
        }
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000)); // alkemis +880
        c.feed_strain(1, U128(1_000)); // alkemis +920 -> 1_800
        set_context(RAIDER, NearToken::from_yoctonear(0));
        c.feed_strain(2, U128(1_000)); // raider +880
        set_context("carol.testnet", NearToken::from_yoctonear(0));
        c.feed_strain(3, U128(1_000)); // carol +880
        set_context("dave.testnet", NearToken::from_yoctonear(0));
        c.feed_strain(4, U128(1_000)); // dave +880 (tidak masuk top-3)
        let top = c.get_top_feeder(0);
        assert_eq!(top.len(), 3, "batas 3 entri");
        assert_eq!(top[0].account_id.to_string(), ALKEMIS_ID);
        assert_eq!(top[0].amount, 880 + 920);
        assert_eq!(top[1].account_id.to_string(), RAIDER);
        assert_eq!(top[2].account_id.to_string(), "carol.testnet");
        assert_eq!(
            c.feast_contributions.get(&(0, ALKEMIS_ID.parse::<AccountId>().unwrap())).copied(),
            Some(880 + 920)
        );
    }

    /// `EN-108` — dormansi: upkeep turun ke 50 bps saat volume < ambang; transisi terukur.
    #[test]
    fn en108_dormansi_upkeep_50_bps() {
        assert_eq!(state::constants::UPKEEP_MIN_BPS, 50);
        let mut c = dk_setup(); // supply 20_000; ambang default 100 bps = 200
        assert!(c.get_appetite_meter().dormant); // volume 0 < 200
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        // f1 = 1_000; rescue 8800 -> 880; dormansi ×50/10000 -> 4.
        assert!(logs.contains(r#""effective_fee":4"#));
        assert!(logs.contains(r#""dormant":true"#));
        assert!(c.get_appetite_meter().dormant); // volume 4 masih < 200
        // Transisi: ambang 0 -> tidak dorman; fee kembali penuh (rescue saja).
        c.macro_state.params.dormancy_threshold_bps = 0;
        assert!(!c.get_appetite_meter().dormant);
        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        c.feed_strain(1, U128(1_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        // rescue eff 9000 -> 920 (tanpa dormansi).
        assert!(logs.contains(r#""effective_fee":920"#));
        assert!(logs.contains(r#""dormant":false"#));
        assert!(c.snacc_balances.get(&1).copied().unwrap() < bal0);
    }

    /// `EN-109` — rescue feeding: batas 0 (fee 20%) dan 100% (fee penuh); linear di antaranya.
    #[test]
    fn en109_rescue_batas_0_dan_100_persen() {
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        let _ = c.snacc_balances.insert(1, 20_000); // saldo cukup untuk fee penuh
        // Batas bawah: eff = 1 -> multiplier = 2000 (RESCUE_MIN_FEE_BPS).
        let mut v = c.strains.get(&1).unwrap().clone();
        v.vitality = 1;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""effective_fee":2000"#)); // 10_000 × 2000/10000
        assert!(logs.contains(r#""rescue_bps":2000"#));

        // Batas atas: eff = 10000 -> multiplier 10000 (fee penuh).
        let mut v = c.strains.get(&1).unwrap().clone();
        v.vitality = constants::VITALITY_CAP; // 10000
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""effective_fee":10000"#));
        assert!(logs.contains(r#""rescue_bps":10000"#));
    }

    /// `EN-110` + `EN-111` — matriks diet per kasta per zona: 8 kombinasi
    /// (mask yang punya bit Fermenter: 0b010/0b011/0b110/0b111 × Fever/Hypo).
    #[test]
    fn en110_en111_matriks_diet_8_kombinasi() {
        // Fever (volume 30_000 -> temp 1500): bit Extractor -> 2000.
        // Hypo (volume 0 -> temp 0): bit Synthesizer -> 2000.
        let cases: [(u8, u128, u16); 8] = [
            (0b010, 30_000, 0),     // Fermenter @Fever: bukan Extractor -> 0
            (0b010, 0, 0),          // Fermenter @Hypo: bukan Synthesizer -> 0
            (0b011, 30_000, 2_000), // E+F @Fever -> 2000
            (0b011, 0, 0),          // E+F @Hypo -> 0
            (0b110, 30_000, 0),     // F+S @Fever -> 0
            (0b110, 0, 2_000),      // F+S @Hypo -> 2000
            (0b111, 30_000, 2_000), // Full @Fever -> 2000
            (0b111, 0, 2_000),      // Full @Hypo -> 2000
        ];
        for (i, (mask, volume, expected)) in cases.iter().enumerate() {
            let mut c = SnaccContract::new();
            c.macro_state.params.dormancy_threshold_bps = 0;
            let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
            let mut v = c.strains.get(&1).unwrap().clone();
            v.role_mask = *mask;
            let _ = c.strains.insert(1, v);
            grant(&mut c, "mt:spore-ignis", 2);
            c.macro_state.volume_epoch = *volume;
            c.macro_state.last_update_ms = 3_600_000_000; // decay 0 (last_update awal 0)
            let effective = 1_000u128 * u128::from(10_000 - *expected) / 10_000;
            set_context(ALKEMIS_ID, NearToken::from_yoctonear(effective));
            let _ = c.transmute_spores_to_enzyme(1, 2, U128(1_000));
            let logs = near_sdk::test_utils::get_logs().join("\n");
            assert!(
                logs.contains(&format!(r#""diet_bps":{}"#, expected)),
                "kasus {}: diet_bps {} tidak tercapai",
                i,
                expected
            );
        }
    }

    /// `EN-110` — attached deposit != fee setelah diskon -> ditolak.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn en110_diet_fee_nominal_ditolak() {
        let mut c = SnaccContract::new();
        c.macro_state.params.dormancy_threshold_bps = 0;
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011; // punya bit Extractor -> diskon Fever
        let _ = c.strains.insert(1, v);
        grant(&mut c, "mt:spore-ignis", 2);
        c.macro_state.volume_epoch = 30_000; // Fever
        c.macro_state.last_update_ms = 3_600_000_000;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000)); // harusnya 800
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(1_000));
    }

    /// `EN-112` — `get_appetite_meter`: zona, countdown burst, status dormansi, feast.
    #[test]
    fn en112_get_appetite_meter() {
        let mut c = dk_setup(); // supply 20_000 -> feast target 1_000
        let m = c.get_appetite_meter();
        assert_eq!(m.zone, "HYPO"); // volume 0 -> temp 0
        assert_eq!(m.crucible_temp, 0);
        assert_eq!(m.current_epoch, 0);
        assert_eq!(m.burst_bps, 0);
        assert_eq!(m.burst_epochs_left, 0);
        assert!(m.dormant);
        assert_eq!(m.feast_volume, 0);
        assert_eq!(m.feast_target, 1_000);
        // Burst aktif -> meter mencerminkan.
        c.macro_state.nutrient_burst_bps = 2_000;
        c.macro_state.nutrient_burst_until = 3;
        let m = c.get_appetite_meter();
        assert_eq!(m.burst_bps, 2_000);
        assert_eq!(m.burst_until, 3);
        assert_eq!(m.burst_epochs_left, 3);
    }

    // ------------------------------------------------------------------
    // TUGAS 3 — SEC-107..109
    // ------------------------------------------------------------------

    // Vektor ed25519 SEGAR (dihasilkan offline & self-verified) untuk alur apply sukses
    // + anti-replay `used_tee_chat_ids` (SEC-107). Model: `z-ai/glm-5.3-flash`.
    const SEC_BODY: &str = r#"{"id":"sec-replay-test","choices":[{"message":{"content":"{\"strain_id\":1,\"efficiency_shift_bps\":100,\"vitality_bonus\":100,\"phenotype_trait\":\"SEC-Replay-Probe\"}"}}]}"#;
    const SEC_REQ_SHA: &str =
        "01c45b8519d21a933ffdf96737428001c6fa720afeadc1c1a6beeb66ba6055d4";
    const SEC_PUBKEY_HEX: &str =
        "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
    const SEC_SIG_HEX: &str = "b22529732798f14ce0e0645d357407ebb71ac6278238e357f0b85edba9ea6584295c52aa1d8dbba9b9fd8d681cb7c772dee0bd6d8f84e45b2208eb1a56f4f405";

    /// `SEC-107` — alur TEE sukses penuh lalu replay `chat_id` yang sama ditolak
    /// (`used_tee_chat_ids`, negative test lanjutan).
    #[test]
    #[should_panic(expected = "ERR_TEE_REPLAY")]
    fn sec107_anti_replay_chat_id_sama_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.register_attested_signer(SEC_PUBKEY_HEX.to_string(), 1_700_000_000);
        c.commit_genome_experiment(1, SEC_REQ_SHA.to_string());
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.apply_tee_mutation_zero_trust(
            1,
            SEC_REQ_SHA.to_string(),
            SEC_BODY.to_string(),
            SEC_SIG_HEX.to_string(),
            SEC_PUBKEY_HEX.to_string(),
        );
        // Mutasi sukses: +100 bps, +100 vitality, trait tercatat.
        assert_eq!(v.efficiency_bps, 10_100);
        assert_eq!(v.vitality, 8_600);
        assert_eq!(v.phenotype_trait.as_deref(), Some("SEC-Replay-Probe"));
        // REPLAY: komit ulang (commitment lama sudah dipakai) lalu body yang SAMA —
        // `chat_id` "sec-replay-test" sudah di `used_tee_chat_ids` -> ditolak.
        c.commit_genome_experiment(1, SEC_REQ_SHA.to_string());
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.apply_tee_mutation_zero_trust(
            1,
            SEC_REQ_SHA.to_string(),
            SEC_BODY.to_string(),
            SEC_SIG_HEX.to_string(),
            SEC_PUBKEY_HEX.to_string(),
        );
    }

    /// `SEC-108` — `state_nonce`/`pending_callbacks` konsisten `ADR-29` pada mutasi baru:
    /// feed & hostile menaikkan nonce; ft_transfer (ledger akun) tidak menyentuh nonce
    /// strain; hostile diblokir saat callback berjalan (dites di hostile_pending_callback).
    #[test]
    fn sec108_state_nonce_konsisten_adr29() {
        let mut c = hostile_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        assert_eq!(c.get_strain(1).state_nonce, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.feed_strain(1, U128(1_000));
        assert_eq!(c.get_strain(1).state_nonce, 1); // feed menaikkan nonce
        // ft_transfer antar akun tidak mengubah nonce StrainVessel mana pun.
        fund(&mut c, ALKEMIS_ID, 100);
        register(&mut c, RAIDER);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(RAIDER.parse().unwrap(), U128(50), None);
        assert_eq!(c.get_strain(1).state_nonce, 1);
        assert_eq!(c.get_strain(2).state_nonce, 0);
        // hostile: kedua nonce naik (`record_mutation` kedua sisi).
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.hostile_takeover_splice(2, 1, 1); // target nonce = 1 (feed)
        assert!(c.get_strain(2).state_nonce > 0);
        assert!(c.get_strain(1).state_nonce > 0);
    }

    /// `SEC-109` — anti-farming `DK-030`: loop 1.000 feed + klaim dividen — saldo akhir
    /// tetap rugi bersih; total rebate ≤ total fee yang dipungut.
    #[test]
    fn sec109_dk030_loop_feed_dan_dividen_merugi_bersih() {
        let mut c = SnaccContract::new();
        // Strain 1 Extractor (pembayar fee) + strain 2 Synthesizer (klaim dividen).
        let _ = call_incubate(&mut c, 1_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000_000_000));
        let _ = c.incubate_strain(role(SYNTHESIZER_ROLE), U128(1_000_000_000), 0);
        c.macro_state.params.dormancy_threshold_bps = 0;
        c.macro_state.epoch_treasury_initial = c.treasury;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        for i in 0..1_000 {
            if i % 10 == 0 {
                set_context(ALKEMIS_ID, NearToken::from_yoctonear(0)); // reset buffer log mock
            }
            c.feed_strain(1, U128(1_000));
        }
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        assert!(bal1 < bal0, "DK-030: loop feed harus rugi bersih");
        assert!(c.macro_state.epoch_treasury_spent <= c.macro_state.epoch_feast_volume);

        // Klaim dividen dari kolam (burn aether + tutup epoch + klaim pro-rata penuh).
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.advance_epoch();
        let payout = c.claim_industrial_dividend(2, 0).0;
        assert!(payout > 0);
        // Meski dividen diklaim penuh, total masih rugi bersih (`DK-030`).
        let bal2 = c.snacc_balances.get(&1).copied().unwrap();
        assert!(bal2 + payout < bal0, "rebate+dividen tidak boleh melampaui fee");
    }
}

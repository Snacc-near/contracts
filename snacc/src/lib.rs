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
    constants, AlkemisProfile, MacroState, MacroStateView, MnaLogEntry, OrganelleSlots, StrainId,
    StrainRole, StrainStatus, StrainVessel, StrainVesselView, TeeAttestation, TokenId,
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
    /// TODO(NEP-141): digabung ke ledger token global saat tersedia.
    pub account_balances: LookupMap<AccountId, u128>,
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
        let _ = self.alchemists.insert(owner_id, profile);

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

    /// `F4` — transisi epoch minimal (`VC-005`): naikkan `current_epoch`, reset
    /// `epoch_treasury_spent`, dan snapshot `epoch_treasury_initial = self.treasury`.
    /// TODO(DK-033): distribusi `industrial_dividend_pool` & penutupan dividen epoch penuh.
    pub fn advance_epoch(&mut self) {
        self.macro_state.current_epoch = self.macro_state.current_epoch.saturating_add(1);
        self.macro_state.epoch_treasury_spent = 0;
        self.macro_state.epoch_treasury_initial = self.treasury;
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
        let now_ms = env::block_timestamp() / 1_000_000;
        let v_decayed = self.macro_state.decayed_volume(now_ms);
        self.macro_state.volume_epoch = v_decayed.saturating_add(fee.0);
        self.macro_state.last_update_ms = now_ms;

        let temp = self.get_crucible_temp();
        if temp > self.macro_state.params.fever_threshold_mk {
            let burn = fee.0 * self.macro_state.params.fever_burn_bps as u128
                / constants::BPS_DENOMINATOR;
            self.macro_state.total_snacc_supply =
                self.macro_state.total_snacc_supply.saturating_sub(burn);
            self.macro_state.total_snacc_burned =
                self.macro_state.total_snacc_burned.saturating_add(burn);
            let remainder = fee.0.saturating_sub(burn);
            self.treasury = self.treasury.saturating_add(remainder);
        } else if temp < self.macro_state.params.hypothermia_threshold_mk {
            let raw_rebate = fee.0 * self.macro_state.params.rebate_bps as u128
                / constants::BPS_DENOMINATOR;
            let max_epoch_drain = self.macro_state.epoch_treasury_initial
                * self.macro_state.params.max_epoch_treasury_drain_bps as u128
                / constants::BPS_DENOMINATOR;
            let remaining_quota =
                max_epoch_drain.saturating_sub(self.macro_state.epoch_treasury_spent);
            let rebate = raw_rebate.min(remaining_quota).min(self.treasury);
            self.treasury = self.treasury.saturating_add(fee.0).saturating_sub(rebate);
            if rebate > 0 {
                let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
                let _ = self
                    .snacc_balances
                    .insert(strain_id, bal.saturating_add(rebate));
                self.macro_state.epoch_treasury_spent =
                    self.macro_state.epoch_treasury_spent.saturating_add(rebate);
            }
        } else {
            self.treasury = self.treasury.saturating_add(fee.0);
        }
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
        self.take_catalyst_fee(strain_id, &snacc_catalyst_fee);
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
            r#"{{"kind":"transmuted","strain_id":{},"burned_spores":{},"token_out":"{}","minted":{},"catalyst_fee":{}}}"#,
            strain_id, spores_to_burn, recipe.0, minted, snacc_catalyst_fee.0
        ));
        self.get_strain(strain_id)
    }

    /// `F2` Tahap 3 — `synthesize_elixir` (wajib `ROLE_SYNTHESIZER`).
    /// `mt:elixir-aether` butuh Attestation TEE (TODO: `F3`/`DK-044`) →
    /// untuk `VC-004` ditolak `ERR_REQUIRES_TEE_ATTESTATION`.
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
            constants::ELIXIR_AETHER => env::panic_str(constants::ERR_REQUIRES_TEE_ATTESTATION),
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
        (U128(bounty), U128(recovery))
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
        // baseline treasury = 20% stake inkubasi (2e11) + katalis 7.
        assert_eq!(c.treasury, 200_000_000_000 + 7);
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
        assert_eq!(c.treasury, treasury_before + (fee - expected_burn));
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
        // Sisa fee masuk treasury (+10_000), rebate keluar dari treasury (-200).
        assert_eq!(c.treasury, treasury_before + fee - expected_rebate);
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
        assert_eq!(c.treasury, treasury_after_cap + 10_000);
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
        assert_eq!(c.treasury, 8_000);

        // Batas atas zona tengah: tambah fee = 8_000 -> volume_epoch = 14_000 -> temp = 1400 mK
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(8_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(8_000));
        assert_eq!(c.get_crucible_temp(), 1400);
        assert_eq!(c.macro_state.total_snacc_burned, 0);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000);
        assert_eq!(c.treasury, 16_000);
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
}

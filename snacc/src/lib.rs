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
    MnaLogEntry, NftMetadataView, NftTokenView, NpcMerchantAgent, NpcQuoteView, OrganelleSlots,
    SolvencyTier, StrainId, StrainRole, StrainStatus, StrainVessel, StrainVesselView, TeeAttestation, TokenId,
};

/// Payload mutasi genom — `choices[0].message.content` (JSON di dalam JSON, `F3`/`I-03`).
/// DK-074 — skema ketat: field asing ditolak (`deny_unknown_fields`).
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct MutationPayload {
    strain_id: u64,
    efficiency_shift_bps: i64,
    vitality_bonus: u64,
    #[serde(default)]
    phenotype_trait: Option<String>,
}

/// DK-074 — `phenotype_trait` maksimal 32 karakter (spec Diagram Konseptual).
/// Daftar putih lengkap belum terdefinisi di vault — panjang ditegakkan di sini;
/// whitelist menyusul bila spec memancang daftarnya (keputusan tercatat).
const TEE_TRAIT_MAX_CHARS: usize = 32;

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

// CATATAN DK-074: envelope/choice (format gateway) sengaja TOLERAN — field gateway
// bisa bertambah (index, finish_reason, logprobs, …) tanpa merusak verifikasi.
// Skema KETAT (deny_unknown_fields) ditegakkan pada MutationPayload — payload
// keputusan itulah yang menjadi permukaan serangan.
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
    /// `DK-033` — `(inflow, aether_burned_total)` per epoch tertutup: dasar payout
    /// eksak `kontribusi × inflow / burned` (mul-div 256-bit, tanpa overflow skala).
    pub epoch_dividend_totals: LookupMap<Epoch, (u128, u128)>,
    /// `DK-033` — `epoch_aether_burned[(epoch, strain_id)] -> amount`.
    pub epoch_aether_burned: LookupMap<(Epoch, StrainId), u128>,
    /// `DK-033` — anti double-claim `(epoch, strain_id)`.
    pub dividend_claimed: LookupSet<(Epoch, StrainId)>,
    /// `EN-107` — kontribusi feed per `(epoch, akun)` (Great Feast).
    pub feast_contributions: LookupMap<(Epoch, AccountId), u128>,
    /// `EN-107` — leaderboard top-3 feeder per epoch (batas 3 entri, gas aman).
    pub epoch_top_feeders: LookupMap<Epoch, Vec<FeederEntry>>,
    /// `ADR-30` — 3 NPC pedagang otonom on-chain (`D4`).
    pub npc_agents: LookupMap<String, NpcMerchantAgent>,
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
    /// SEC-112/migrate — generasi prefix collection (naik tiap `migrate()`;
    /// collection baru memakai `[gen, base]` → data lama tak terbaca).
    pub prefix_gen: u8,
    /// `ADR-22` — jam sandbox demo aktif? Diset SEKALI di `new()`/`migrate()` dari
    /// akun kontrak (hanya akun berakhiran `.testnet`); tidak ada setter → mustahil
    /// aktif di mainnet.
    pub sandbox_clock_enabled: bool,
    /// `ADR-22` — `sandbox_time_offset_sec` ditambahkan ke waktu blok oleh `now_sec()`.
    pub sandbox_time_offset_sec: u64,
    /// Audit W8 — tag generasi state untuk nama sub-account `s-<id>-<tag>`:
    /// tinggi blok saat `new()`/`migrate()` (naik monoton) → `next_strain_id` yang
    /// mulai ulang setelah reset state tidak bentrok dengan `s-*` sisa deployment lama.
    pub subaccount_tag: u64,
}

const GEN_MARKER: &[u8] = b"__snacc_prefix_gen_v1";
/// SEC-113 — marker owner raw-storage (bertahan lintas migrate, di luar collection).
const OWNER_MARKER: &[u8] = b"__snacc_owner_v1";

/// `floor(a × b / d)` dengan hasil kali 256-bit (tanpa overflow antara);
/// hasil > u128 → saturasi `u128::MAX`. `d == 0` → 0 (pemanggil memakai `max(d,1)`).
pub(crate) fn mul_div_u128(a: u128, b: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    const M: u128 = u64::MAX as u128;
    let (a1, a0) = (a >> 64, a & M);
    let (b1, b0) = (b >> 64, b & M);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 64) + (p01 & M) + (p10 & M);
    let lo = (p00 & M) | ((mid & M) << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    if hi == 0 {
        return lo / d;
    }
    if hi >= d {
        return u128::MAX;
    }
    // Pembagian panjang 256/128 (hi < d menjamin hasil muat u128).
    let mut rem = hi;
    let mut q: u128 = 0;
    for i in (0..128).rev() {
        let carry = rem >> 127;
        rem = (rem << 1) | ((lo >> i) & 1);
        q <<= 1;
        if carry == 1 || rem >= d {
            rem = rem.wrapping_sub(d);
            q |= 1;
        }
    }
    q
}
/// DK-073 — batas ukuran respons TEE (spec: "mis. ≤4 KB").
const TEE_MAX_RESPONSE_BYTES: usize = 4096;

#[near]
impl SnaccContract {
    /// Bangun contract state dengan prefix `[gen, base]` — SEC-112/migrate.
    fn build(gen: u8) -> Self {
        let px = |b: u8| vec![gen, b];
        let mut c = Self {
            prefix_gen: gen,
            alchemists: LookupMap::new(px(97)),
            strains: LookupMap::new(px(115)),
            inventories: LookupMap::new(px(105)),
            tee_attestations: LookupMap::new(px(116)),
            used_tee_chat_ids: LookupSet::new(px(117)),
            attested_tee_signers: LookupMap::new(px(112)),
            owner_id: None,
            account_balances: LookupMap::new(px(108)),
            strains_by_owner: LookupMap::new(px(111)),
            registered_storage: LookupMap::new(px(114)),
            epoch_dividend_rate: LookupMap::new(px(101)),
            epoch_dividend_totals: LookupMap::new(px(99)),
            epoch_aether_burned: LookupMap::new(px(102)),
            dividend_claimed: LookupSet::new(px(103)),
            feast_contributions: LookupMap::new(px(104)),
            epoch_top_feeders: LookupMap::new(px(107)),
            npc_agents: LookupMap::new(px(110)),
            snacc_balances: LookupMap::new(px(98)),
            treasury: 0,
            macro_state: MacroState::default(),
            mna_logs: Vector::new(px(109)),
            next_strain_id: 1,
            sandbox_clock_enabled: Self::is_sandbox_network(),
            sandbox_time_offset_sec: 0,
            subaccount_tag: env::block_height(),
        };
        // Audit W6 — epoch 0 berakhir `EPOCH_DURATION_SEC` setelah init/migrate.
        c.macro_state.epoch_ends_sec = c.now_sec() + constants::EPOCH_DURATION_SEC;
        c
    }

    /// Audit W8 — nama sub-account Strain: `s-<id>-<subaccount_tag>.<akun-kontrak>`
    /// (suffix = akun kontrak: parent selalu ada di jaringannya — Pipeline #7).
    fn subaccount_for(&self, strain_id: StrainId) -> AccountId {
        format!(
            "s-{}-{}.{}",
            strain_id,
            self.subaccount_tag,
            env::current_account_id()
        )
        .parse()
        .unwrap()
    }

    /// `ADR-22` — gerbang mode sandbox: hanya akun kontrak `*.testnet`.
    fn is_sandbox_network() -> bool {
        env::current_account_id().as_str().ends_with(".testnet")
    }

    /// `ADR-22` — SATU-SATUNYA sumber waktu (detik) untuk BMR/aging/epoch/zona:
    /// waktu blok + `sandbox_time_offset_sec` (0 di luar mode sandbox).
    pub(crate) fn now_sec(&self) -> u64 {
        env::block_timestamp() / 1_000_000_000 + self.sandbox_time_offset_sec
    }

    /// Varian milidetik dari `now_sec()` (offset sandbox ikut).
    pub(crate) fn now_ms(&self) -> u64 {
        env::block_timestamp() / 1_000_000 + self.sandbox_time_offset_sec * 1_000
    }

    /// `advance_sandbox_clock` (`ADR-22`, Spec Antarmuka §1): menggeser
    /// `sandbox_time_offset_sec += hours * 3600` agar juri dapat menguji Chapter 11 &
    /// Chapter 7 dalam 1 klik. Hanya pemilik kontrak, hanya mode sandbox (`.testnet`).
    pub fn advance_sandbox_clock(&mut self, hours: u64) -> u64 {
        assert!(
            self.sandbox_clock_enabled,
            "{}",
            constants::ERR_SANDBOX_CLOCK_DISABLED
        );
        let caller = env::predecessor_account_id();
        assert_eq!(
            Some(&caller),
            self.owner_id.as_ref(),
            "{}",
            constants::ERR_NOT_CONTRACT_OWNER
        );
        assert!(
            hours > 0 && hours <= constants::MAX_SANDBOX_ADVANCE_HOURS,
            "{}",
            constants::ERR_BAD_AMOUNT
        );
        self.sandbox_time_offset_sec = self
            .sandbox_time_offset_sec
            .saturating_add(hours * 3600);
        events::emit_event_json(&format!(
            r#"{{"kind":"sandbox_clock_advanced","hours":{},"sandbox_time_offset_sec":{}}}"#,
            hours, self.sandbox_time_offset_sec
        ));
        self.sandbox_time_offset_sec
    }

    /// View mode sandbox: `(sandbox_clock_enabled, sandbox_time_offset_sec, now_sec)`.
    pub fn get_sandbox_clock(&self) -> (bool, u64, u64) {
        (
            self.sandbox_clock_enabled,
            self.sandbox_time_offset_sec,
            self.now_sec(),
        )
    }

    #[init]
    pub fn new() -> Self {
        // gen=0 → prefix [0, base] — sudah BERBEDA dari kontrak pra-gen (bare [base])
        // sehingga data lama tak terbaca meski tanpa marker. Marker hanya untuk migrate.
        // SEC-113 — owner ditetapkan SAAT deploy (deployer), bukan penelepon pertama
        // method mana pun (polanya membuka pengambilalihan owner).
        let mut c = Self::build(0);
        c.owner_id = Some(env::predecessor_account_id());
        env::storage_write(OWNER_MARKER, c.owner_id.as_ref().unwrap().as_bytes());
        c.spawn_npcs();
        c
    }

    /// `ADR-30` — seed 3 NPC otonom (`D4` Model Data): stok genesis = stok target
    /// (keputusan tercatat), reserve 0, harga dasar `NPC_P_BASE`.
    fn spawn_npcs(&mut self) {
        for (npc_id, archetype, target) in [
            ("npc-extractor", "Pyre-X", constants::NPC_TARGET_EXTRACTOR),
            ("npc-central-arb", "Myco-7", constants::NPC_TARGET_CENTRAL_ARB),
            ("npc-vulture-ma", "Void-0", constants::NPC_TARGET_VULTURE),
        ] {
            let _ = self.npc_agents.insert(
                npc_id.to_string(),
                NpcMerchantAgent {
                    npc_id: npc_id.to_string(),
                    wallet_account: format!("{}.snacc-v001.near", npc_id),
                    role_archetype: archetype.to_string(),
                    snacc_reserve: 0,
                    spores_stock: target,
                    target_spores_stock: target,
                    base_spore_price: constants::NPC_P_BASE,
                    trades_executed: 0,
                },
            );
        }
    }

    /// Penanda versi protokol — dipakai smoke test & verifikasi deploy.
    pub fn protocol_version(&self) -> &'static str {
        "snacc-v001"
    }

    /// `F1` / `P1` — cetak `Homunculus Strain` Genesis (`NEP-171`).
    ///
    /// `initial_snacc_stake` adalah nominal split `80/20`. Sumber stake wajib nyata:
    /// attached NEAR (nominal `0` atau == deposit; suplai lahir dari stake NEAR) atau,
    /// bila deposit `0`, debit saldo `$SNACC` akun pemanggil (tanpa mint baru).
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

        // Audit W5/W7 — sumber stake WAJIB nyata (tutup mint bebas, `I-01`):
        // (a) attached NEAR > 0 → suplai lahir dari stake NEAR (keputusan tercatat:
        //     faucet & `ft_transfer_call` superseded; jalur onboarding pemain baru);
        //     argumen 0 atau == deposit.
        // (b) attached 0 → stake didebit dari saldo `$SNACC` akun pemanggil
        //     (`account_balances`: bounty lisis / payout M&A) — suplai TIDAK bertambah.
        let attached = env::attached_deposit().as_yoctonear();
        let caller = env::predecessor_account_id();
        let (stake, mint_new_supply): (u128, bool) = if attached > 0 {
            assert!(
                initial_snacc_stake.0 == 0 || initial_snacc_stake.0 == attached,
                "{}", constants::ERR_STAKE_DEPOSIT_MISMATCH
            );
            (attached, true)
        } else {
            assert!(
                initial_snacc_stake.0 > 0,
                "{}", constants::ERR_INSUFFICIENT_DEPOSIT
            );
            let bal = self.account_balance(&caller);
            assert!(
                bal >= initial_snacc_stake.0,
                "{}", constants::ERR_INSUFFICIENT_BALANCE
            );
            self.set_account_balance(&caller, bal - initial_snacc_stake.0);
            (initial_snacc_stake.0, false)
        };

        // `F1`: `20%` fee → `Rule-Bound Treasury` (`D6`),
        // `80%` → modal kerja Strain (sisa pembulatan menjaga konservasi `I-01`).
        let to_treasury = stake * constants::STAKE_TREASURY_BPS / constants::BPS_DENOMINATOR;
        let to_strain = stake - to_treasury;

        let owner_id = caller;
        let now_sec = self.now_sec();
        let strain_id = self.next_strain_id;
        self.next_strain_id += 1;

        // Pipeline #7 — suffix diturunkan dari akun kontrak (parent SELALU ada):
        // hardcode ".snacc-v001.near" gagal di testnet (parent tak terdaftar ->
        // `create_account` pasti gagal on-chain).
        let vault_subaccount: AccountId = self.subaccount_for(strain_id);

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
            atp_balance: constants::INITIAL_ATP,
            pending_commitment: None,
            phenotype_trait: None,
            generation: 0,
            organel: OrganelleSlots::default(),
            last_allocate_epoch: u64::MAX,
            status: StrainStatus::Active,
        };

        // `D2` ledger, `D6` neraca internal & treasury, `D7` suplai `$SNACC` (`I-01`).
        let _ = self.strains.insert(strain_id, vessel.clone());
        self.set_strain_snacc(strain_id, to_strain);
        self.treasury += to_treasury;
        if mint_new_supply {
            self.macro_state.total_snacc_supply += stake;
        }
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

        // `EN-124` — counter demografi: inkubasi = birth.
        self.macro_state.total_births = self.macro_state.total_births.saturating_add(1);
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
                let mut owned = Vector::new(format!("o:{}:{}", self.prefix_gen, owner_id).into_bytes());
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
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
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

        // SEC-111 (triase Opsi a) — kunci `pending_callbacks` menjadi NYATA: set 1
        // selama promise window; `assert_atomic_ready` menolak mutasi M&A di tengah
        // window; reset 0 di callback `on_subaccount_created` (semua jalur).
        let mut locked = vessel.clone();
        locked.pending_callbacks = 1;
        locked.record_mutation();
        let _ = self.strains.insert(strain_id, locked);

        // Audit W8 — nama dari ledger (unik lintas generasi state; lihat `subaccount_for`).
        let sub_id: AccountId = vessel.vault_subaccount.clone();

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
        // SEC-111 — lepas kunci `pending_callbacks` di SEMUA jalur (window tertutup).
        if let Some(mut v) = self.strains.get(&strain_id).cloned() {
            v.pending_callbacks = 0;
            v.record_mutation();
            let _ = self.strains.insert(strain_id, v);
        }
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
        vessel.to_view(self.now_sec())
    }

    /// `get_solvency_tier` (Spec Antarmuka §2): `Tier1Healthy` (`>2000`),
    /// `Tier2Chapter11Distressed` (`1..=2000`), `Tier3Chapter7Insolvent` (`0`).
    pub fn get_solvency_tier(&self, strain_id: StrainId) -> SolvencyTier {
        self.strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .solvency_tier(self.now_sec())
    }

    // =====================================================================
    // VC-005 — `F4`: Termostat Makro `CrucibleTemp` (`ADR-04`, `I-01`, `I-04`)
    // =====================================================================

    /// `F4` / `ADR-04` — suhu makro `CrucibleTemp` (`mK`) dengan lazy decay tanpa cron:
    /// `floor(1000 * V_decayed / max(total_snacc_supply, 1))`.
    pub fn get_crucible_temp(&self) -> u64 {
        let now_ms = self.now_ms();
        self.macro_state.crucible_temp_at(now_ms)
    }

    /// `F4` — view `MacroState` + `crucible_temp` terhitung pada timestamp saat ini.
    pub fn get_macro_state(&self) -> MacroStateView {
        let now_ms = self.now_ms();
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
        // Audit W6 — gerbang waktu: epoch hanya bisa ditutup setelah berakhir
        // (anti-spam: reset kuota I-04, tutup dividen, kedaluwarsa klaim).
        let now_sec = self.now_sec();
        assert!(
            now_sec >= self.macro_state.epoch_ends_sec,
            "{}",
            constants::ERR_EPOCH_NOT_ENDED
        );
        // Epoch berikut dihitung dari `now` (keeper terlambat tidak bisa menutup
        // beberapa epoch beruntun dalam satu blok).
        self.macro_state.epoch_ends_sec = now_sec + constants::EPOCH_DURATION_SEC;
        // --- `DK-033`: penutupan epoch dividen (V-02: waktu dari blok, bukan parameter). ---
        let closing = self.macro_state.current_epoch;
        let total_burned = self.macro_state.epoch_aether_burned_total;
        // Bug live (tx gagal `advance_epoch`): `inflow × 1e18` overflow u128 untuk
        // inflow ≥ ~3.4e20 → mul-div 256-bit. `rate` hanya informatif (saturasi di
        // u128::MAX); payout klaim memakai `epoch_dividend_totals` (eksak).
        let inflow = self.macro_state.epoch_dividend_inflow;
        let rate = mul_div_u128(
            inflow,
            constants::DIVIDEND_RATE_SCALE,
            std::cmp::max(total_burned, 1),
        );
        let _ = self.epoch_dividend_rate.insert(closing, rate);
        let _ = self
            .epoch_dividend_totals
            .insert(closing, (inflow, total_burned));
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
        self.set_strain_snacc(strain_id, bal - amount.0);
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

    /// Pipeline #6 — SATU-SATUNYA jalur tulis `snacc_balances`: memelihara agregat
    /// `sum_strain_snacc` untuk invarian `V-04` (LookupMap tak bisa diiterasi).
    fn set_strain_snacc(&mut self, strain_id: StrainId, new_bal: u128) {
        let old = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        let delta = new_bal as i128 - old as i128;
        self.macro_state.sum_strain_snacc =
            (self.macro_state.sum_strain_snacc as i128 + delta).max(0) as u128;
        let _ = self.snacc_balances.insert(strain_id, new_bal);
    }

    /// Pipeline #6 — jalur tulis `account_balances` + agregat `sum_account_balances`.
    fn set_account_balance(&mut self, account: &AccountId, new_bal: u128) {
        let old = self.account_balances.get(account).copied().unwrap_or(0);
        let delta = new_bal as i128 - old as i128;
        self.macro_state.sum_account_balances =
            (self.macro_state.sum_account_balances as i128 + delta).max(0) as u128;
        let _ = self.account_balances.insert(account.clone(), new_bal);
    }

    /// `EN-114`/§13a — potong ATP strain; `ERR_NOT_ENOUGH_ATP` bila kurang.
    /// ATP non-transferable: hanya dipakai sebagai biaya kerja internal.
    fn pay_atp(v: &mut StrainVessel, cost: u128) {
        assert!(
            v.atp_balance >= cost,
            "{}",
            constants::ERR_NOT_ENOUGH_ATP
        );
        v.atp_balance -= cost;
    }

    /// `EN-115`/§13a — `atp_minted = (fee / ATP_YOCTO_UNIT) × efficiency_bps/10000
    /// × (10000 + 1000 × level_mitokondria)/10000` (mitokondria +10%/level).
    /// Catatan: komponen `zona` pada EN-06 tidak termasuk (§13a final tanpa zona).
    fn calc_atp_minted(fee_yocto: u128, efficiency_bps: u16, level_mitokondria: u8) -> u128 {
        let base = fee_yocto / constants::ATP_YOCTO_UNIT;
        base * u128::from(efficiency_bps) / constants::BPS_DENOMINATOR
            * u128::from(10_000 + 1_000 * u16::from(level_mitokondria))
            / constants::BPS_DENOMINATOR
    }

    /// `ECON-111` FINAL — tambah kaki K2/K3 `V_epoch` (econ-report-3 §1):
    /// `unit × VOLUME_SCALE(5) × P_BASE(100)` untuk mt_burn & mt_mint.
    /// K4 (kas likuidasi) ditambahkan langsung di `harvest_starving_strain`.
    fn add_volume_legs(&mut self, burn_units: u128, mint_units: u128) {
        let legs = (burn_units + mint_units)
            * constants::VOLUME_SCALE
            * constants::P_BASE;
        self.macro_state.volume_epoch = self.macro_state.volume_epoch.saturating_add(legs);
    }

    /// Gerbang `Lazy BMR` (`F2`): tolak bila `effective_vitality == 0` (`ERR_STRAIN_STARVING`).
    fn assert_not_starving(v: &StrainVessel, now_sec: u64) -> u64 {
        let eff = v.effective_vitality(now_sec);
        assert!(eff > 0, "{}", constants::ERR_STRAIN_STARVING);
        eff
    }

    /// `V-01` (audit W4) — hanya pemilik Strain yang boleh memakai ATP/inventori/
    /// metabolismenya (`ERR_NOT_STRAIN_OWNER`, Spec Dividend §5).
    fn assert_strain_owner(v: &StrainVessel) {
        assert_eq!(
            env::predecessor_account_id(),
            v.owner_id,
            "{}",
            constants::ERR_NOT_STRAIN_OWNER
        );
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
        // Pipeline #6 — fee EKSTERNAL (attached NEAR-yocto) = inflow nyata untuk V-04;
        // fee feed_strain INTERNAL tidak dihitung (lihat catatan SEC-112/VI di atas).
        self.macro_state.total_catalyst_fees = self
            .macro_state
            .total_catalyst_fees
            .saturating_add(fee.0);
        self.apply_thermostat_fee(strain_id, fee.0, 0);
    }

    /// Jalur termostat untuk fee internal (`feed_strain` `EN-103`): sama seperti
    /// `take_catalyst_fee` tanpa asersi attached deposit.
    fn apply_thermostat_fee(&mut self, strain_id: StrainId, fee: u128, extra_volume: u128) {
        // CATATAN SEC-112/VI: `total_catalyst_fees` hanya menghitung fee EKSTERNAL
        // (attached yocto — lewat `take_catalyst_fee`); fee `feed_strain` INTERNAL
        // (dari saldo strain) tidak dihitung inflow (sudah konservasi internal).
        let now_ms = self.now_ms();
        let v_decayed = self.macro_state.decayed_volume(now_ms);
        // EN-117 (keputusan pemilik: suhu literal ATP) — `extra_volume` =
        // `atp_minted × ATP_VOLUME_SCALE` dari feed; masuk V_epoch epoch yang sama.
        self.macro_state.volume_epoch = v_decayed
            .saturating_add(fee)
            .saturating_add(extra_volume);
        self.macro_state.last_update_ms = now_ms;

        let temp = self.get_crucible_temp();
        // `EN-106` — deteksi transisi zona makro → event `zone_changed` (tanpa keeper).
        let zone = self.zone_at(temp);
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
            // `LC-11` — pelindung `mt:elixir-homeostasis` (katalog molekul): pemegang
            // >=1 unit mendapat diskon 50% pada burn Fever — PER STRAIN (keputusan
            // tercatat; eliksir tidak dikonsumsi, efek pasif).
            let burn = if self.inv_balance(strain_id, constants::ELIXIR_HOMEOSTASIS) >= 1 {
                fee * u128::from(self.macro_state.params.fever_burn_bps) * 50
                    / (100 * constants::BPS_DENOMINATOR)
            } else {
                fee * u128::from(self.macro_state.params.fever_burn_bps)
                    / constants::BPS_DENOMINATOR
            };
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
                    self.set_strain_snacc(strain_id, bal.saturating_add(rebate));
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
        let now_sec = self.now_sec();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        Self::assert_strain_owner(&v);
        let eff = Self::assert_not_starving(&v, now_sec);
        Self::assert_role(&v, constants::ROLE_EXTRACTOR);
        assert!(
            constants::SPORE_TOKEN_IDS.contains(&spore_type.as_str()),
            "{}",
            constants::ERR_UNKNOWN_TOKEN
        );
        // `EN-116`/§13a — biaya ATP extract = 1.
        Self::pay_atp(&mut v, 1);
        self.take_catalyst_fee(strain_id, &snacc_catalyst_fee);
        self.inv_mint(strain_id, &spore_type, 1);
        // `ECON-111` — kaki K3: mt_mint 1 unit spora.
        self.add_volume_legs(0, 1);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"extract","cost":1}}"#, strain_id
        ));
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
        let now_sec = self.now_sec();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        Self::assert_strain_owner(&v);
        let eff = Self::assert_not_starving(&v, now_sec);
        Self::assert_role(&v, constants::ROLE_FERMENTER);
        assert!(
            spores_to_burn > 0 && spores_to_burn.is_multiple_of(2),
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
        let diet_bps: u16 = match self.zone_at(temp_now) {
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
        // `EN-116`/§13a — biaya ATP transmute = 2.
        Self::pay_atp(&mut v, 2);
        self.take_catalyst_fee(strain_id, &U128(effective_fee));
        for (token, amount) in recipe.1 {
            if amount > 0 {
                self.inv_burn(strain_id, token, amount);
            }
        }
        // EN-119 — Nukleus: output produksi +10%/level (keputusan pemilik 2026-11-08).
        let nuk_bps = u128::from(v.organel.nukleus)
            * constants::NUCLEUS_OUTPUT_BPS_PER_LEVEL;
        let minted = burn
            * u128::from(v.efficiency_bps)
            * (constants::BPS_DENOMINATOR + nuk_bps)
            / (constants::BPS_DENOMINATOR * constants::BPS_DENOMINATOR);
        self.inv_mint(strain_id, recipe.0, minted);
        // `ECON-111` — kaki K2 (spora dibakar) & K3 (enzim di-mint).
        self.add_volume_legs(burn, minted);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"transmute","cost":2}}"#, strain_id
        ));
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
        let now_sec = self.now_sec();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        Self::assert_strain_owner(&v);
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
            enzymes_to_burn > 0 && (enzymes_to_burn as u128).is_multiple_of(batch_total),
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
        // `EN-116`/§13a — biaya ATP synthesize = 3.
        Self::pay_atp(&mut v, 3);
        self.take_catalyst_fee(strain_id, &snacc_catalyst_fee);
        self.inv_burn(strain_id, enz_a, need_a);
        self.inv_burn(strain_id, enz_b, need_b);
        // EN-119 — Nukleus: output eliksir +10%/level (keputusan pemilik).
        let nuk_bps = u128::from(v.organel.nukleus)
            * constants::NUCLEUS_OUTPUT_BPS_PER_LEVEL;
        let minted_elixir = batches
            * (constants::BPS_DENOMINATOR + nuk_bps)
            / constants::BPS_DENOMINATOR;
        self.inv_mint(strain_id, &elixir_type, minted_elixir);
        // `ECON-111` — kaki K2 (enzim dibakar) & K3 (eliksir di-mint).
        self.add_volume_legs(enzymes_to_burn as u128, batches);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"synthesize","cost":3}}"#, strain_id
        ));
        Self::recover_metabolism(&mut v, eff, now_sec);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"synthesized","strain_id":{},"token_out":"{}","minted":{},"enzymes_burned":{},"catalyst_fee":{}}}"#,
            strain_id, elixir_type, minted_elixir, enzymes_to_burn, snacc_catalyst_fee.0
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
        if let Some(trait_name) = &payload.phenotype_trait {
            assert!(
                trait_name.chars().count() <= TEE_TRAIT_MAX_CHARS,
                "{}",
                constants::ERR_TEE_TRAIT_INVALID
            );
        }
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
        // DK-073 — batas ukuran respons diperiksa PALING AWAL (sebelum hash/sig)
        // agar body raksasa tidak menghabiskan gas hash/parse.
        assert!(
            raw_response_body.len() <= TEE_MAX_RESPONSE_BYTES,
            "{}",
            constants::ERR_TEE_RESPONSE_TOO_LARGE
        );
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
    /// SEC-113 — hanya pemilik kontrak (owner ditetapkan saat deploy/migrate);
    /// pola first-caller-becomes-owner DIHAPUS. Menimpa kunci terdaftar dengan
    /// attestation berbeda ditolak (tidak diam-diam, `I-16`).
    pub fn register_attested_signer(&mut self, pubkey_hex: String, attested_at_sec: u64) {
        let caller = env::predecessor_account_id();
        assert_eq!(
            Some(&caller),
            self.owner_id.as_ref(),
            "{}",
            constants::ERR_NOT_CONTRACT_OWNER
        );
        let pk = Self::hex_to_bytes::<32>(&pubkey_hex);
        if let Some(old) = self.attested_tee_signers.get(&pk) {
            assert_eq!(
                *old, attested_at_sec,
                "{}",
                constants::ERR_TEE_SIGNER_OVERWRITE
            );
        }
        let _ = self.attested_tee_signers.insert(pk, attested_at_sec);
        // DK-070 — event `signer_added` (nama persis dari spec Diagram Konseptual).
        events::emit_event_json(&format!(
            r#"{{"kind":"signer_added","pubkey_hex":"{}","attested_at_sec":{}}}"#,
            pubkey_hex, attested_at_sec
        ));
    }

    /// DK-070 — `remove_attested_signer`: hanya pemilik kontrak; kunci harus
    /// terdaftar; event `signer_removed` (nama persis dari spec).
    pub fn remove_attested_signer(&mut self, pubkey_hex: String) {
        let caller = env::predecessor_account_id();
        assert_eq!(
            Some(&caller),
            self.owner_id.as_ref(),
            "{}",
            constants::ERR_NOT_CONTRACT_OWNER
        );
        let pk = Self::hex_to_bytes::<32>(&pubkey_hex);
        assert!(
            self.attested_tee_signers.get(&pk).is_some(),
            "{}",
            constants::ERR_TEE_SIGNER_NOT_FOUND
        );
        let _ = self.attested_tee_signers.remove(&pk);
        events::emit_event_json(&format!(
            r#"{{"kind":"signer_removed","pubkey_hex":"{}"}}"#,
            pubkey_hex
        ));
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
    /// TODO(EN-120): warisan set organel + wajib realokasi ATP; `MAX_GENERATION` ditegakkan (audit W10).
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
        // Audit W10 — status DUA sel dicek (dulu hanya `absorbed`): dominan yang
        // sudah Liquidated/SplicedBurned tidak boleh menyerap sel lain.
        for s in [&dom.status, &abs.status] {
            assert!(
                !matches!(s, StrainStatus::SplicedBurned | StrainStatus::Liquidated),
                "{}",
                constants::ERR_STRAIN_ALREADY_SPLICED
            );
        }
        // Audit W10 — dominan Tier 3 (vitalitas efektif 0) tidak bisa melebur
        // (jalur lisis permissionless tidak boleh dihindari lewat fusi).
        let now_sec = self.now_sec();
        let dom_eff = Self::assert_not_starving(&dom, now_sec);
        // `EN-124` — `MAX_GENERATION` ditegakkan di semua jalur pembuat generasi baru.
        let new_gen = dom.generation.saturating_add(1);
        assert!(
            new_gen <= constants::MAX_GENERATION,
            "{}",
            constants::ERR_MAX_GENERATION
        );
        // Pajak merger `2.5%` (`250 bps`) dari saldo `absorbed` (`ADR-25`).
        let abs_bal = self.snacc_balances.get(&absorbed_id).copied().unwrap_or(0);
        let tax = abs_bal * constants::MERGER_TAX_BPS as u128 / constants::BPS_DENOMINATOR;
        self.treasury = self.treasury.saturating_add(tax);
        let dom_bal = self.snacc_balances.get(&dominant_id).copied().unwrap_or(0);
        self.set_strain_snacc(dominant_id, dom_bal.saturating_add(abs_bal - tax));
        self.set_strain_snacc(absorbed_id, 0);
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
        dom.generation = new_gen;
        dom.efficiency_bps = dom.efficiency_bps.saturating_add(800);
        // LC-06 — fusi: `BMR −15%` (`SPLICE_BMR_REDUCTION_BPS`). Vitalitas efektif
        // di-settle dulu ke `now` agar BMR baru tidak berlaku mundur (Lazy BMR).
        dom.vitality = dom_eff;
        dom.last_fed_sec = now_sec;
        dom.bmr_per_hour = dom.bmr_per_hour
            * (10_000 - constants::SPLICE_BMR_REDUCTION_BPS)
            / 10_000;
        // `EN-116`/§13a — biaya ATP merger = 5 (dipotong dari dominan).
        Self::pay_atp(&mut dom, 5);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"splice","cost":5}}"#, dominant_id
        ));
        // `EN-120` — endosimbiosis: tiap slot organel = max(dominan, terserap);
        // realokasi ATP dibuka kembali (`last_allocate_epoch` di-reset).
        dom.organel = OrganelleSlots {
            mitokondria: dom.organel.mitokondria.max(abs.organel.mitokondria),
            nukleus: dom.organel.nukleus.max(abs.organel.nukleus),
            lisosom: dom.organel.lisosom.max(abs.organel.lisosom),
        };
        dom.last_allocate_epoch = u64::MAX;
        abs.status = StrainStatus::SplicedBurned;
        dom.record_mutation();
        abs.record_mutation();
        let _ = self.strains.insert(dominant_id, dom);
        let _ = self.strains.insert(absorbed_id, abs);
        // `EN-124` — counter demografi: splice = merger.
        self.macro_state.total_mergers = self.macro_state.total_mergers.saturating_add(1);
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
        let now_sec = self.now_sec();
        let eff = target.effective_vitality(now_sec);
        assert!(
            (1..=2000).contains(&eff),
            "{}",
            constants::ERR_NOT_DISTRESSED
        );
        // `I-14` (`ADR-29`) — anti ERC-6551 seller-drain (nonce + callback lock).
        target.assert_atomic_ready(expected_target_nonce);
        // Audit W10 / `EN-124` — `MAX_GENERATION` juga di jalur hostile; raider
        // yang sudah Liquidated/SplicedBurned tidak bisa menyerap.
        assert!(
            !matches!(
                raider.status,
                StrainStatus::SplicedBurned | StrainStatus::Liquidated
            ),
            "{}",
            constants::ERR_STRAIN_ALREADY_SPLICED
        );
        assert!(
            raider.generation.saturating_add(1) <= constants::MAX_GENERATION,
            "{}",
            constants::ERR_MAX_GENERATION
        );

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
        self.set_strain_snacc(raider_strain_id, raider_bal - payment + target_bal);
        self.set_strain_snacc(distressed_target_id, 0);
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
        // `EN-116`/§13a — biaya ATP takeover = 10 (dipotong dari raider).
        Self::pay_atp(&mut raider, 10);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"hostile","cost":10}}"#, raider_strain_id
        ));
        // `EN-120` — endosimbiosis hostile: organel = max per slot; realokasi ATP
        // dibuka kembali (`last_allocate_epoch` di-reset).
        raider.organel = OrganelleSlots {
            mitokondria: raider.organel.mitokondria.max(target.organel.mitokondria),
            nukleus: raider.organel.nukleus.max(target.organel.nukleus),
            lisosom: raider.organel.lisosom.max(target.organel.lisosom),
        };
        raider.last_allocate_epoch = u64::MAX;
        target.status = StrainStatus::SplicedBurned;
        raider.record_mutation();
        target.record_mutation();
        let _ = self.strains.insert(raider_strain_id, raider.clone());
        let _ = self.strains.insert(distressed_target_id, target);

        // `D8` — log M&A.
        self.mna_logs.push(MnaLogEntry {
            timestamp_ms: self.now_ms(),
            kind: state::MnaKind::HostileTakeover,
            actor_id: caller,
            target_id: distressed_target_id,
            amount: payment,
        });
        // `EN-124` — counter demografi: hostile takeover = merger.
        self.macro_state.total_mergers = self.macro_state.total_mergers.saturating_add(1);
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
        let now_sec = self.now_sec();
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
        self.set_account_balance(&caller, caller_bal + bounty);
        self.treasury = self.treasury.saturating_add(recovery);
        self.set_strain_snacc(target_id, 0);
        // Inventori molekul dibakar habis (DK-054).
        for token in constants::ALL_TOKEN_IDS {
            let _ = self.inventories.insert((target_id, token.to_string()), 0);
        }
        v.status = StrainStatus::Liquidated;
        v.record_mutation();
        let _ = self.strains.insert(target_id, v);
        // `EN-124` — counter demografi: harvest = lysis.
        self.macro_state.total_lyses = self.macro_state.total_lyses.saturating_add(1);
        // `ECON-111` — kaki K4: kas yang berpindah pada likuidasi (bounty + recovery).
        self.macro_state.volume_epoch = self
            .macro_state
            .volume_epoch
            .saturating_add(bounty + recovery);
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
    /// Zona makro dari suhu memakai threshold **params** (`ECON-111` final: Fever >610,
    /// Hypothermia <600, Normal [600,610]) — selalu konsisten dengan termostat.
    fn zone_at(&self, temp_mk: u64) -> MacroZone {
        if temp_mk > self.macro_state.params.fever_threshold_mk {
            MacroZone::Fever
        } else if temp_mk < self.macro_state.params.hypothermia_threshold_mk {
            MacroZone::Hypothermia
        } else {
            MacroZone::Normal
        }
    }

    /// Nama zona untuk SVG/event (kanonik string `FEVER`/`HYPO`/`NORMAL`).
    fn zone_of(&self, temp_mk: u64) -> &'static str {
        self.zone_at(temp_mk).as_str()
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
        let now_ms = self.now_ms();
        let now_sec = self.now_sec();
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
            self.zone_of(temp),
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
            self.set_account_balance(&sender_id, sender_bal - amount.0);
            let recv = self.account_balance(&receiver_id);
            self.set_account_balance(&receiver_id, recv.saturating_add(amount.0));
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
            self.set_account_balance(&sender_id, bal.saturating_add(unused));
            // Sisa dana ditarik kembali dari penerima (mengembalikan konservasi).
            let recv_bal = self.account_balance(&receiver_id);
            self.set_account_balance(&receiver_id, recv_bal.saturating_sub(unused));
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
        let now_sec = self.now_sec();
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
    #[payable]
    pub fn claim_industrial_dividend(&mut self, strain_id: StrainId, epoch: Epoch) -> U128 {
        // Pipeline #7 — payable defensif (refund deposit menempel; lihat feed_strain).
        let attached = env::attached_deposit();
        if attached > NearToken::from_near(0) {
            let _ = Promise::new(env::predecessor_account_id()).transfer(attached);
        }
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
        // Payout eksak `kontribusi × inflow / max(burned, 1)` (256-bit, tanpa overflow);
        // fallback ke `rate` untuk epoch yang ditutup sebelum totals disimpan.
        let mut payout = match self.epoch_dividend_totals.get(&epoch).copied() {
            Some((inflow, burned)) => {
                mul_div_u128(contribution, inflow, std::cmp::max(burned, 1))
            }
            None => {
                let rate = self.epoch_dividend_rate.get(&epoch).copied().unwrap_or(0);
                mul_div_u128(contribution, rate, constants::DIVIDEND_RATE_SCALE)
            }
        };
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
        self.set_strain_snacc(strain_id, bal.saturating_add(payout));
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
    #[payable]
    pub fn feed_strain(&mut self, strain_id: StrainId, amount: U128) -> StrainVesselView {
        // Pipeline #7 — payable defensif: deposit yang menempel dikembalikan penuh
        // (method non-payable memicu trap wasm tanpa pesan bila operator menempel deposit).
        let attached = env::attached_deposit();
        if attached > NearToken::from_near(0) {
            let _ = Promise::new(env::predecessor_account_id()).transfer(attached);
        }
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, v.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert!(amount.0 > 0, "{}", constants::ERR_INSUFFICIENT_BALANCE);
        let now_sec = self.now_sec();
        let now_ms = self.now_ms();
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

        // DK-030 (triase temuan EN-104) — LANTAI FEE: kombinasi burst × dormansi ×
        // rescue tidak boleh menghasilkan 0 (feed gratis = anti-farming lolos).
        // Lantai = 1 yocto: selalu ada fee > 0. Formula lain tidak diubah.
        let effective_fee = ((base_fee - discount) * rescue_bps / constants::BPS_DENOMINATOR
            * upkeep_bps
            / constants::BPS_DENOMINATOR)
            .max(1);

        // EN-117 (suhu literal ATP) — atp_minted dihitung dari fee efektif SEBELUM
        // termostat agar `atp_minted × ATP_VOLUME_SCALE` masuk V_epoch epoch yang sama.
        let atp_minted = Self::calc_atp_minted(
            effective_fee,
            v.efficiency_bps,
            v.organel.mitokondria,
        );
        let atp_volume = atp_minted.saturating_mul(constants::ATP_VOLUME_SCALE);

        // Debit `effective_fee` dari saldo strain -> jalur termostat `F4`.
        self.set_strain_snacc(strain_id, bal - effective_fee);
        self.apply_thermostat_fee(strain_id, effective_fee, atp_volume);

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

        // `EN-06`/§13a — ATP dicetak dari fee efektif (dihitung di atas, masuk
        // V_epoch via `atp_volume` — suhu literal ATP EN-117).
        v.atp_balance = v.atp_balance.saturating_add(atp_minted);
        // Pemulihan metabolik (pola `F2`) + nonce.
        Self::recover_metabolism(&mut v, eff, now_sec);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"fed","strain_id":{},"base_fee":{},"effective_fee":{},"burst_bps":{},"rescue_bps":{},"dormant":{},"atp_minted":{}}}"#,
            strain_id, base_fee, effective_fee, burst_bps, rescue_bps, dormant, atp_minted
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
        top.sort_by_key(|f| std::cmp::Reverse(f.amount));
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
        let now_ms = self.now_ms();
        let temp = self.macro_state.crucible_temp_at(now_ms);
        let zone = self.zone_at(temp);
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
            risk_heat: self.get_risk_heat(),
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
    /// `EN-125` — `SpeculativeFlux` (0..10000; tingkat pertaruhan epoch berjalan).
    pub risk_heat: u64,
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
    /// SEC-112 — migrate dengan PREFIX GENERASI: marker `__snacc_prefix_gen_v1`
    /// (raw storage key di luar semua collection) dibaca → gen+1 → SEMUA collection
    /// memakai prefix `[gen, base]` yang baru → data lama (prefix lama) tak terbaca.
    /// `#[init(ignore_state)]` melewati deserialisasi struct lama (layout berubah aman).
    /// Old data jadi unreachable garbage (biaya storage rent, bukan kebocoran logika).
    #[init(ignore_state)]
    #[private]
    pub fn migrate() -> Self {
        // SEC-113 — `#[private]`: hanya akun kontrak sendiri yang boleh memicu
        // migrate (penelepon lain → seluruh state tak terbaca + owner reset).
        let old_gen = env::storage_read(GEN_MARKER)
            .and_then(|b| b.first().copied())
            .unwrap_or(0);
        let gen = old_gen.saturating_add(1);
        env::storage_write(GEN_MARKER, &[gen]);
        let mut c = Self::build(gen);
        // SEC-113 — owner lama dipertahankan lewat marker raw-storage; fallback:
        // pemanggil private (= akun kontrak sendiri).
        let owner = env::storage_read(OWNER_MARKER)
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<AccountId>().ok())
            .unwrap_or_else(env::predecessor_account_id);
        c.owner_id = Some(owner.clone());
        env::storage_write(OWNER_MARKER, owner.as_bytes());
        c.spawn_npcs();
        c
    }
}

// =====================================================================
// Pipeline #3 — EN-121/122/123 (koloni, sitokinesis, adopsi) + EN-125 + NPC ADR-30
// =====================================================================
#[near]
impl SnaccContract {
    /// Hitung jumlah strain AKTIF milik akun (koloni; `Liquidated`/`SplicedBurned`
    /// tidak dihitung — pagar `MAX_COLONY_SIZE` untuk sel hidup).
    fn colony_active_count(&self, owner: &AccountId) -> u64 {
        self.strains_by_owner
            .get(owner)
            .map(|v| {
                v.iter()
                    .filter(|id| {
                        self.strains
                            .get(id)
                            .map(|s| {
                                !matches!(
                                    s.status,
                                    StrainStatus::Liquidated | StrainStatus::SplicedBurned
                                )
                            })
                            .unwrap_or(false)
                    })
                    .count() as u64
            })
            .unwrap_or(0)
    }

    /// `EN-121` — koloni satu akun (sel hidup, ≤ `MAX_COLONY_SIZE` by construction).
    /// TODO(sesi-utama): Σ ATP koloni = Σ `atp_balance` (field EN-114) — view
    /// `get_colony_atp(owner_id)` ditambahkan setelah field ATP ter-merge (EN-121:
    /// "anggaran $ATP bersama koloni").
    pub fn get_colony(&self, owner_id: AccountId) -> Vec<StrainVesselView> {
        let now_sec = self.now_sec();
        let mut out = Vec::new();
        if let Some(owned) = self.strains_by_owner.get(&owner_id) {
            for id in owned.iter() {
                if let Some(v) = self.strains.get(id) {
                    if !matches!(
                        v.status,
                        StrainStatus::Liquidated | StrainStatus::SplicedBurned
                    ) {
                        out.push(v.to_view(now_sec));
                    }
                }
            }
        }
        out
    }

    /// `EN-121` — Σ ATP koloni aktif satu akun (anggaran `$ATP` bersama, LC-20).
    pub fn get_colony_atp(&self, owner_id: AccountId) -> U128 {
        let mut total: u128 = 0;
        if let Some(owned) = self.strains_by_owner.get(&owner_id) {
            for id in owned.iter() {
                if let Some(v) = self.strains.get(id) {
                    if !matches!(
                        v.status,
                        StrainStatus::Liquidated | StrainStatus::SplicedBurned
                    ) {
                        total = total.saturating_add(v.atp_balance);
                    }
                }
            }
        }
        U128(total)
    }

    /// `EN-122` — `divide_strain`: sitokinesis (§13a). Saldo, vitality, dan spora
    /// `mt:spore-ignis` anak = separuh (induk dipotong — Σ induk+anak = saldo induk);
    /// organel ÷2 (anak floor, induk sisa — total terjaga); `generation` anak = induk+1;
    /// biaya ATP 3; pagar `MAX_COLONY_SIZE=5` / `MAX_GENERATION=8`.
    pub fn divide_strain(&mut self, strain_id: StrainId) -> (StrainVesselView, StrainVesselView) {
        let caller = env::predecessor_account_id();
        let mut parent = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, parent.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert!(
            !matches!(
                parent.status,
                StrainStatus::Liquidated | StrainStatus::SplicedBurned
            ),
            "{}",
            constants::ERR_STRAIN_ALREADY_SPLICED
        );
        // Pagar koloni/generasi (`EN-124`/§13a).
        assert!(
            self.colony_active_count(&caller) < u64::from(constants::MAX_COLONY_SIZE),
            "{}",
            constants::ERR_COLONY_FULL
        );
        let child_gen = parent.generation.saturating_add(1);
        assert!(
            child_gen <= constants::MAX_GENERATION,
            "{}",
            constants::ERR_MAX_GENERATION
        );
        // Induk hidup (anakan tidak lahir dari sel mati).
        let now_sec = self.now_sec();
        let _ = Self::assert_not_starving(&parent, now_sec);
        // `EN-116`/§13a — biaya ATP sitokinesis = 3.
        Self::pay_atp(&mut parent, 3);
        // `EN-124` — invariant demografi protektif: pembelahan (reproduksi gratis)
        // wajib punya "anggaran" dari merger + lisis. Interpretasi tercatat: bentuk
        // literal `births ≤ merges + lyses` membuat inkubasi pertama mustahil
        // (births=1, merges+lyses=0), maka gate diterapkan pada divides:
        // `total_divides ≤ total_merges + total_lyses` (inkubasi berbayar bebas —
        // modal nyata masuk, `V-04` aman). Dilaporkan di laporan pipeline #5.
        self.macro_state.total_divides = self.macro_state.total_divides.saturating_add(1);
        // births = inkubasi + divide (§13a/brief).
        self.macro_state.total_births = self.macro_state.total_births.saturating_add(1);
        assert!(
            self.macro_state.total_divides
                <= self.macro_state.total_mergers + self.macro_state.total_lyses,
            "{}",
            constants::ERR_BIRTHS_EXCEEDED
        );
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"divide","cost":3}}"#, strain_id
        ));

        // Pembagian kas: anak = floor(bal/2), induk = sisa (Σ = saldo induk, `I-01`).
        let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        let child_bal = bal / 2;
        self.set_strain_snacc(strain_id, bal - child_bal);
        // Pembagian vitality: anak separuh, induk dipotong.
        let child_vitality = parent.vitality / 2;
        parent.vitality -= child_vitality;
        // Pembagian spora `mt:spore-ignis`: anak = floor(n/2), induk = sisa.
        let ignis = self.inv_balance(strain_id, constants::GENESIS_SPORE_TOKEN_ID);
        let child_spores = ignis / 2;
        let _ = self
            .inventories
            .insert((strain_id, constants::GENESIS_SPORE_TOKEN_ID.to_string()), ignis - child_spores);

        // Organel ÷2 (anak floor per slot, induk sisa — total terjaga).
        let child_organel = OrganelleSlots {
            mitokondria: parent.organel.mitokondria / 2,
            nukleus: parent.organel.nukleus / 2,
            lisosom: parent.organel.lisosom / 2,
        };
        parent.organel = OrganelleSlots {
            mitokondria: parent.organel.mitokondria - child_organel.mitokondria,
            nukleus: parent.organel.nukleus - child_organel.nukleus,
            lisosom: parent.organel.lisosom - child_organel.lisosom,
        };

        // Anakan: genom (role_mask, efficiency, phenotype) & bmr diwariskan; gen = induk+1.
        let child_id = self.next_strain_id;
        self.next_strain_id += 1;
        self.set_strain_snacc(child_id, child_bal);
        if child_spores > 0 {
            self.inv_mint(child_id, constants::GENESIS_SPORE_TOKEN_ID, child_spores);
        }
        let child = StrainVessel {
            strain_id: child_id,
            owner_id: caller.clone(),
            vault_subaccount: self.subaccount_for(child_id),
            vitality: child_vitality,
            efficiency_bps: parent.efficiency_bps,
            bmr_per_hour: parent.bmr_per_hour,
            last_fed_sec: now_sec,
            role_mask: parent.role_mask,
            state_nonce: 0,
            pending_callbacks: 0,
            // §13a — ATP anak = `INITIAL_ATP` (50, energi kelahiran).
            atp_balance: constants::INITIAL_ATP,
            pending_commitment: None,
            phenotype_trait: parent.phenotype_trait.clone(),
            generation: child_gen,
            organel: child_organel,
            last_allocate_epoch: u64::MAX,
            status: StrainStatus::Active,
        };
        let _ = self.strains.insert(child_id, child.clone());
        parent.record_mutation();
        let _ = self.strains.insert(strain_id, parent.clone());
        // Indeks koloni: anak masuk daftar pemilik.
        match self.strains_by_owner.get_mut(&caller) {
            Some(owned) => owned.push(child_id),
            None => {
                let mut owned = near_sdk::store::Vector::new(
                    format!("o:{}:{}", self.prefix_gen, caller).into_bytes(),
                );
                owned.push(child_id);
                let _ = self.strains_by_owner.insert(caller.clone(), owned);
            }
        }
        // `D8` log + event.
        self.mna_logs.push(MnaLogEntry {
            timestamp_ms: self.now_ms(),
            kind: state::MnaKind::Divide,
            actor_id: caller,
            target_id: child_id,
            amount: child_bal,
        });
        events::emit_event_json(&format!(
            r#"{{"kind":"strain_divided","parent_id":{},"child_id":{},"child_generation":{},"parent_balance":{},"child_balance":{},"child_vitality":{},"child_spores":{}}}"#,
            strain_id, child_id, child_gen, bal - child_bal, child_bal, child_vitality, child_spores
        ));
        (parent.to_view(now_sec), child.to_view(now_sec))
    }

    /// `EN-123` — `adopt`: ambil alih Strain Distressed (`1 <= eff <= 2000`) dengan
    /// `book_value` **TANPA premium 15%** → 100% ke pemilik lama; ownership pindah
    /// ke pemilik raider (mencegah lisis — kompetisi dengan `hostile_takeover_splice`).
    pub fn adopt(
        &mut self,
        target_id: StrainId,
        raider_strain_id: StrainId,
        expected_target_nonce: u64,
    ) -> StrainVesselView {
        assert_ne!(target_id, raider_strain_id, "{}", constants::ERR_SAME_STRAIN);
        let caller = env::predecessor_account_id();
        let raider = self
            .strains
            .get(&raider_strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, raider.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        let mut target = self
            .strains
            .get(&target_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert!(
            !matches!(
                target.status,
                StrainStatus::Liquidated | StrainStatus::SplicedBurned
            ),
            "{}",
            constants::ERR_ALREADY_LIQUIDATED
        );
        // `SEC-110`/`I-14` — anti seller-drain: nonce target wajib cocok (pola hostile).
        target.assert_atomic_ready(expected_target_nonce);
        let now_sec = self.now_sec();
        let eff = target.effective_vitality(now_sec);
        // Jendela Distressed (Tier 2); eff 0 = Tier 3 → jalur F6 (lisis), bukan adopt.
        assert!((1..=2000).contains(&eff), "{}", constants::ERR_NOT_DISTRESSED);
        // Koloni pengadopsi wajib punya ruang (sel hidup pindah koloni).
        assert!(
            self.colony_active_count(&caller) < u64::from(constants::MAX_COLONY_SIZE),
            "{}",
            constants::ERR_COLONY_FULL
        );
        // Book value (saldo + Σ inventori × floor_price 1) TANPA premium → 100% ke pemilik lama.
        let target_bal = self.snacc_balances.get(&target_id).copied().unwrap_or(0);
        let mut book_value = target_bal;
        for token in constants::ALL_TOKEN_IDS {
            book_value = book_value.saturating_add(
                self.inv_balance(target_id, token) * constants::MOLECULE_FLOOR_PRICE,
            );
        }
        let payer_bal = self.snacc_balances.get(&raider_strain_id).copied().unwrap_or(0);
        assert!(
            payer_bal >= book_value,
            "{}",
            constants::ERR_INSUFFICIENT_BALANCE
        );
        let old_owner = target.owner_id.clone();
        self.set_strain_snacc(raider_strain_id, payer_bal - book_value);
        let seller = self.account_balances.get(&old_owner).copied().unwrap_or(0);
        self.set_account_balance(&old_owner, seller.saturating_add(book_value));

        // Ownership pindah: perbarui indeks `strains_by_owner` (keluar dari koloni lama).
        if let Some(owned) = self.strains_by_owner.get_mut(&old_owner) {
            if let Some(idx) = owned.iter().position(|id| *id == target_id) {
                owned.swap_remove(idx as u32);
            }
        }
        match self.strains_by_owner.get_mut(&caller) {
            Some(owned) => owned.push(target_id),
            None => {
                let mut owned =
                    near_sdk::store::Vector::new(format!("o:{}:{}", self.prefix_gen, caller).into_bytes());
                owned.push(target_id);
                let _ = self.strains_by_owner.insert(caller.clone(), owned);
            }
        }
        target.owner_id = caller;
        target.record_mutation();
        let _ = self.strains.insert(target_id, target.clone());
        // `D8` log + event.
        let adopter = raider.owner_id.clone();
        self.mna_logs.push(MnaLogEntry {
            timestamp_ms: self.now_ms(),
            kind: state::MnaKind::Adopt,
            actor_id: adopter.clone(),
            target_id,
            amount: book_value,
        });
        events::emit_event_json(&format!(
            r#"{{"kind":"strain_adopted","target_id":{},"adopter":"{}","raider_strain_id":{},"book_value":{}}}"#,
            target_id, adopter, raider_strain_id, book_value
        ));
        target.to_view(now_sec)
    }

    /// `LC-20` — `conjugate_traits`: konjugasi plasmid — **salin** `phenotype_trait`
    /// dari strain sumber ke strain target TANPA merger (sumber tetap memilikinya).
    /// Syarat (keputusan tercatat): pemilik sama (`ERR_NOT_SAME_OWNER`), kedua sel
    /// hidup, parameter `phenotype_trait` wajib cocok dengan trait sumber saat ini
    /// (mencegah kejutan TOCTOU — `ERR_TRAIT_MISMATCH`); biaya ATP target = 2
    /// (keputusan: §13a tidak mendaftarkan konjugasi, mengikuti biaya transmute).
    pub fn conjugate_traits(
        &mut self,
        from_strain_id: StrainId,
        to_strain_id: StrainId,
        phenotype_trait: String,
    ) -> StrainVesselView {
        assert_ne!(
            from_strain_id, to_strain_id,
            "{}",
            constants::ERR_SAME_STRAIN
        );
        let caller = env::predecessor_account_id();
        let mut from = self
            .strains
            .get(&from_strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        let mut to = self
            .strains
            .get(&to_strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, from.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert_eq!(caller, to.owner_id, "{}", constants::ERR_NOT_SAME_OWNER);
        assert!(
            !matches!(
                from.status,
                StrainStatus::Liquidated | StrainStatus::SplicedBurned
            ),
            "{}",
            constants::ERR_STRAIN_ALREADY_SPLICED
        );
        assert!(
            !matches!(
                to.status,
                StrainStatus::Liquidated | StrainStatus::SplicedBurned
            ),
            "{}",
            constants::ERR_STRAIN_ALREADY_SPLICED
        );
        assert_eq!(
            from.phenotype_trait.as_deref(),
            Some(phenotype_trait.as_str()),
            "{}",
            constants::ERR_TRAIT_MISMATCH
        );
        // Biaya ATP konjugasi = 2, dibayar strain target (keputusan tercatat).
        Self::pay_atp(&mut to, 2);
        to.phenotype_trait = from.phenotype_trait.clone();
        to.record_mutation();
        from.record_mutation();
        let now_sec = self.now_sec();
        let _ = self.strains.insert(from_strain_id, from);
        let _ = self.strains.insert(to_strain_id, to.clone());
        events::emit_event_json(&format!(
            r#"{{"kind":"trait_conjugated","from_strain_id":{},"to_strain_id":{},"phenotype_trait":"{}"}}"#,
            from_strain_id, to_strain_id, phenotype_trait
        ));
        to.to_view(now_sec)
    }

    /// `EN-125` — `SpeculativeFlux` surfaced: indikator "tingkat pertaruhan" per epoch
    /// (skala 0..10000). Definisi (keputusan tercatat — tidak ternominalisasi di vault):
    /// dasar = proksimitas Fever (`temp × 10000 / 1400`), +1000 saat Nutrient Burst
    /// aktif (momen taruhan EN-01), +1000 saat kolam dividen ≥ 1% supply (momen klaim
    /// DK-033). Cap 10000.
    pub fn get_risk_heat(&self) -> u64 {
        let now_ms = self.now_ms();
        let temp = self.macro_state.crucible_temp_at(now_ms);
        let mut heat = temp.saturating_mul(10_000)
            / self.macro_state.params.fever_threshold_mk;
        if self.macro_state.active_burst_bps(self.macro_state.current_epoch) > 0 {
            heat += 1_000;
        }
        if self.macro_state.industrial_dividend_pool * 100
            >= self.macro_state.total_snacc_supply
        {
            heat += 1_000;
        }
        heat.min(10_000)
    }

    /// `ADR-30` — view satu NPC.
    pub fn get_npc(&self, npc_id: String) -> NpcMerchantAgent {
        self.npc_agents
            .get(&npc_id)
            .cloned()
            .expect(constants::ERR_NPC_UNKNOWN)
    }

    /// Kuotasi harga spora NPC (murni, dipakai quote & buy):
    /// `unit = max(1, floor(P_base × S_target / max(S_after, 1)))` (`I-15` — harga
    /// menaik saat stok NPC menyusut di bawah target; equality memenuhi "≥ floor").
    fn npc_quote_internal(&self, npc: &NpcMerchantAgent, buy_amount: u64) -> NpcQuoteView {
        let s_after = npc.spores_stock.saturating_sub(buy_amount);
        let unit_price = std::cmp::max(
            1,
            npc.base_spore_price * u128::from(npc.target_spores_stock)
                / std::cmp::max(u128::from(s_after), 1),
        );
        let total_cost = unit_price * u128::from(buy_amount);
        NpcQuoteView {
            npc_id: npc.npc_id.clone(),
            unit_price_vita: unit_price,
            total_cost_snacc: total_cost,
            npc_spores_stock_after: s_after,
            crucible_temp_mk: self.get_crucible_temp(),
            reasoning_tooltip: format!(
                "I hold {} Spore-Ignis (target {}). Selling {} drops my stock to {} — unit price rises to {} (P_base {}). Scarcity is my leverage.",
                npc.spores_stock,
                npc.target_spores_stock,
                buy_amount,
                s_after,
                unit_price,
                npc.base_spore_price
            ),
        }
    }

    /// `ADR-30` — kuotasi beli spora dari NPC (view; `now_sec` dari spec §5.1 tidak
    /// dijadikan parameter — `V-02`, konsisten dengan keputusan `hostile_takeover_splice`).
    pub fn npc_quote_spores(&self, npc_id: String, buy_amount: u64) -> NpcQuoteView {
        assert!(buy_amount > 0, "{}", constants::ERR_BAD_AMOUNT);
        let npc = self
            .npc_agents
            .get(&npc_id)
            .cloned()
            .expect(constants::ERR_NPC_UNKNOWN);
        self.npc_quote_internal(&npc, buy_amount)
    }

    /// `ADR-30` / `I-15` — beli atomik spora `mt:spore-ignis` dari NPC dengan kas
    /// saldo strain. Konservasi persis: `Δnpc.snacc_reserve == −Δstrain.balance ==
    /// total_cost` dan `Δnpc.spores_stock == −Δstrain inventory == buy_amount`.
    pub fn npc_buy_spores(
        &mut self,
        strain_id: StrainId,
        npc_id: String,
        buy_amount: u64,
    ) -> NpcQuoteView {
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, v.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert!(buy_amount > 0, "{}", constants::ERR_BAD_AMOUNT);
        let mut npc = self
            .npc_agents
            .get(&npc_id)
            .cloned()
            .expect(constants::ERR_NPC_UNKNOWN);
        assert!(
            npc.spores_stock >= buy_amount,
            "{}",
            constants::ERR_NPC_OUT_OF_STOCK
        );
        let quote = self.npc_quote_internal(&npc, buy_amount);
        let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        assert!(
            bal >= quote.total_cost_snacc,
            "{}",
            constants::ERR_INSUFFICIENT_BALANCE
        );
        // Konservasi `I-15` — dua sisi sama persis (+ agregat `sum_npc_reserve`).
        self.set_strain_snacc(strain_id, bal - quote.total_cost_snacc);
        npc.snacc_reserve = npc.snacc_reserve.saturating_add(quote.total_cost_snacc);
        self.macro_state.sum_npc_reserve = self
            .macro_state
            .sum_npc_reserve
            .saturating_add(quote.total_cost_snacc);
        npc.spores_stock -= buy_amount;
        npc.trades_executed = npc.trades_executed.saturating_add(1);
        let _ = self.npc_agents.insert(npc_id.clone(), npc);
        self.inv_mint(strain_id, constants::GENESIS_SPORE_TOKEN_ID, u128::from(buy_amount));
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"npc_spores_bought","strain_id":{},"npc_id":"{}","amount":{},"unit_price":{},"total_cost":{}}}"#,
            strain_id, npc_id, buy_amount, quote.unit_price_vita, quote.total_cost_snacc
        ));
        quote
    }
}

// =====================================================================
// Pipeline #4 — EN-114..119: metabolisme ATP/mitokondria + kalibrasi ECON-111
// =====================================================================
#[near]
impl SnaccContract {
    /// `EN-118`/§13a — `allocate_atp`: alokasi ATP ke slot organel.
    /// 1× per epoch (`ERR_ALREADY_ALLOCATED`); level ≤ `MAX_ORGANEL_LEVEL = 5`
    /// (`ERR_ORGANEL_LEVEL_MAX`); bayar 1 ATP per KENAIKAN level; set organel,
    /// `last_allocate_epoch = epoch`, `record_mutation`, emit `atp_allocated`.
    pub fn allocate_atp(
        &mut self,
        strain_id: StrainId,
        mitokondria: u8,
        nukleus: u8,
        lisosom: u8,
    ) -> StrainVesselView {
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        assert_eq!(caller, v.owner_id, "{}", constants::ERR_NOT_OWNED_BY_CALLEE);
        assert!(
            !matches!(
                v.status,
                StrainStatus::Liquidated | StrainStatus::SplicedBurned
            ),
            "{}",
            constants::ERR_STRAIN_ALREADY_SPLICED
        );
        assert!(
            mitokondria <= constants::MAX_ORGANEL_LEVEL
                && nukleus <= constants::MAX_ORGANEL_LEVEL
                && lisosom <= constants::MAX_ORGANEL_LEVEL,
            "{}",
            constants::ERR_ORGANEL_LEVEL_MAX
        );
        let epoch = self.macro_state.current_epoch;
        assert!(
            v.last_allocate_epoch != epoch,
            "{}",
            constants::ERR_ALREADY_ALLOCATED
        );
        // Biaya: 1 ATP per kenaikan level per slot (§13a "allocate 1/level").
        let cost = u128::from(mitokondria.saturating_sub(v.organel.mitokondria))
            + u128::from(nukleus.saturating_sub(v.organel.nukleus))
            + u128::from(lisosom.saturating_sub(v.organel.lisosom));
        Self::pay_atp(&mut v, cost);
        v.organel = OrganelleSlots { mitokondria, nukleus, lisosom };
        v.last_allocate_epoch = epoch;
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_allocated","strain_id":{},"mitokondria":{},"nukleus":{},"lisosom":{},"cost":{},"epoch":{}}}"#,
            strain_id, mitokondria, nukleus, lisosom, cost, epoch
        ));
        self.get_strain(strain_id)
    }
}

// =====================================================================
// Pipeline #5 — LC-16: transplantasi sel utuh (`NEP-171` nft_transfer)
// =====================================================================
#[near]
impl SnaccContract {
    /// Pindah entri indeks koloni: keluar dari daftar `from`, masuk daftar `to`.
    fn move_strain_owner_index(
        &mut self,
        strain_id: StrainId,
        from: &AccountId,
        to: &AccountId,
    ) {
        if let Some(owned) = self.strains_by_owner.get_mut(from) {
            if let Some(idx) = owned.iter().position(|id| *id == strain_id) {
                owned.swap_remove(idx as u32);
            }
        }
        match self.strains_by_owner.get_mut(to) {
            Some(owned) => owned.push(strain_id),
            None => {
                let mut owned =
                    near_sdk::store::Vector::new(format!("o:{}:{}", self.prefix_gen, to).into_bytes());
                owned.push(strain_id);
                let _ = self.strains_by_owner.insert(to.clone(), owned);
            }
        }
    }

    /// `LC-16` — `nft_transfer` standar `NEP-171`: transplantasi sel utuh — saldo
    /// `$SNACC` (perut), spora/inventori, organel, ATP, dan `state_nonce` mengikuti
    /// vessel (semua state ber-key `strain_id`). Wajib tepat 1 yoctoNEAR.
    /// `approval_id` diterima demi bentuk ABI NEP-171; registri approval `NEP-178`
    /// tidak diimplementasi (keputusan tercatat — parameter diabaikan).
    /// Sel mati (`Liquidated`/`SplicedBurned`) tidak dapat dipindah.
    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<String>,
        memo: Option<String>,
    ) {
        // NEP-178 (approval management) belum diimplementasi — parameter diterima
        // demi bentuk ABI NEP-171 dan diabaikan (keputusan tercatat).
        let _ = approval_id;
        Self::assert_one_yocto();
        assert_ne!(
            receiver_id,
            env::predecessor_account_id(),
            "{}",
            constants::ERR_SAME_STRAIN
        );
        let strain_id: StrainId = token_id
            .parse()
            .unwrap_or_else(|_| env::panic_str(constants::ERR_STRAIN_NOT_FOUND));
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        let old_owner = v.owner_id.clone();
        assert_eq!(
            env::predecessor_account_id(),
            old_owner,
            "{}",
            constants::ERR_NOT_OWNED_BY_CALLEE
        );
        assert!(
            !matches!(
                v.status,
                StrainStatus::Liquidated | StrainStatus::SplicedBurned
            ),
            "{}",
            constants::ERR_ALREADY_LIQUIDATED
        );
        v.owner_id = receiver_id.clone();
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        self.move_strain_owner_index(strain_id, &old_owner, &receiver_id);
        // Event `NEP-297` standard `nep171` — bentuk persis standar NEP-171.
        events::emit_standard_event(
            "nep171",
            "nft_transfer",
            &format!(
                r#"{{"old_owner_id":"{}","new_owner_id":"{}","token_ids":["{}"],"approved_account_ids":null,"memo":{}}}"#,
                old_owner,
                receiver_id,
                token_id,
                match memo {
                    Some(m) => format!(r#""{}""#, m),
                    None => "null".to_string(),
                }
            ),
        );
    }

    /// `LC-16` — `nft_transfer_call` standar NEP-171: transfer + promise
    /// `receiver.nft_on_transfer` + callback `#[private] nft_resolve_transfer`
    /// yang mengembalikan token bila kontrak penerima menolak / gagal.
    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        msg: String,
        approval_id: Option<String>,
        memo: Option<String>,
    ) -> Promise {
        self.nft_transfer(receiver_id.clone(), token_id.clone(), approval_id, memo);
        // SEC-111 (triase Opsi a) — kunci `pending_callbacks` selama promise window;
        // dilepas di `nft_resolve_transfer` (semua jalur).
        let strain_id: StrainId = token_id
            .parse()
            .unwrap_or_else(|_| env::panic_str(constants::ERR_STRAIN_NOT_FOUND));
        if let Some(mut v) = self.strains.get(&strain_id).cloned() {
            v.pending_callbacks = 1;
            v.record_mutation();
            let _ = self.strains.insert(strain_id, v);
        }
        let sender_id = env::predecessor_account_id();
        let args = near_sdk::serde_json::to_vec(&near_sdk::serde_json::json!({
            "sender_id": sender_id,
            "previous_owner_id": sender_id,
            "token_id": token_id,
            "msg": msg,
        }))
        .unwrap();
        Promise::new(receiver_id.clone())
            .function_call(
                "nft_on_transfer".to_string(),
                args,
                NearToken::from_near(0),
                near_sdk::Gas::from_tgas(20),
            )
            .then(Self::ext(env::current_account_id()).nft_resolve_transfer(
                sender_id,
                receiver_id,
                token_id,
            ))
    }

    /// `ADR-24` — resolusi `nft_transfer_call` (anti-spoofing): kontrak penerima
    /// mengembalikan `bool` (`true` = ditolak); promise gagal = dianggap ditolak;
    /// token kembali ke pemilik sebelumnya (indeks koloni ikut).
    #[private]
    pub fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: String,
    ) {
        // SEC-111 — lepas kunci `pending_callbacks` di SEMUA jalur (diterima/ditolak).
        let strain_id_lock: StrainId = token_id
            .parse()
            .unwrap_or_else(|_| env::panic_str(constants::ERR_STRAIN_NOT_FOUND));
        if let Some(mut v) = self.strains.get(&strain_id_lock).cloned() {
            v.pending_callbacks = 0;
            v.record_mutation();
            let _ = self.strains.insert(strain_id_lock, v);
        }
        let rejected = match env::promise_result_checked(0, usize::MAX) {
            Ok(bytes) => near_sdk::serde_json::from_slice::<bool>(&bytes).unwrap_or(true),
            Err(_) => true,
        };
        if !rejected {
            return;
        }
        let strain_id: StrainId = token_id
            .parse()
            .unwrap_or_else(|_| env::panic_str(constants::ERR_STRAIN_NOT_FOUND));
        if let Some(mut v) = self.strains.get(&strain_id).cloned() {
            let current_owner = v.owner_id.clone();
            v.owner_id = previous_owner_id.clone();
            v.record_mutation();
            let _ = self.strains.insert(strain_id, v);
            self.move_strain_owner_index(strain_id, &current_owner, &previous_owner_id);
            events::emit_standard_event(
                "nep171",
                "nft_transfer",
                &format!(
                    r#"{{"old_owner_id":"{}","new_owner_id":"{}","token_ids":["{}"],"approved_account_ids":null,"memo":"nft_transfer_call rejected"}}"#,
                    receiver_id, previous_owner_id, token_id
                ),
            );
        }
    }
}

// =====================================================================
// Pipeline #6 — view yang hilang (temuan audit KPI J3.8/J3.9):
// `get_inventory` (NEP-245) & `get_global_solvency` (invarian `V-04`)
// =====================================================================
#[near]
impl SnaccContract {
    /// J3.8 — saldo satu molekul `NEP-245` pada strain. `0` bila tidak ada —
    /// TIDAK panic (strain/token tak dikenal = saldo nol, keputusan tercatat).
    pub fn get_inventory(&self, strain_id: StrainId, token_id: String) -> U128 {
        U128(self.inv_balance(strain_id, &token_id))
    }

    /// J3.9 — invarian `V-04` versi lengkap (termasuk ledger akun & NPC).
    /// Rumus persis (keputusan tercatat; seluruhnya integer — toleransi = 0):
    ///   pemasukan = `total_minted (= supply + burned) + total_catalyst_fees`
    ///   pengeluaran = `total_burned + treasury + industrial_dividend_pool
    ///                + epoch_dividend_inflow + Σsaldo strain + Σsaldo akun + Σnpc_reserve`
    /// `drift = pemasukan − pengeluaran`; `solvent = (drift == 0)`.
    /// `drift ≠ 0` = bug pembukuan (dilaporkan jujur, bukan disembunyikan).
    /// Eksklusi (ledger terpisah, bukan `$SNACC`): deposit storage NEP-141,
    /// deposit `materialize_subaccount` (NEAR nyata ke sub-akun).
    pub fn get_global_solvency(&self) -> GlobalSolvencyView {
        let total_minted = self
            .macro_state
            .total_snacc_supply
            .saturating_add(self.macro_state.total_snacc_burned);
        let total_out = self
            .macro_state
            .total_snacc_burned
            .saturating_add(self.treasury)
            .saturating_add(self.macro_state.industrial_dividend_pool)
            .saturating_add(self.macro_state.epoch_dividend_inflow)
            .saturating_add(self.macro_state.sum_strain_snacc)
            .saturating_add(self.macro_state.sum_account_balances)
            .saturating_add(self.macro_state.sum_npc_reserve);
        let total_in = total_minted.saturating_add(self.macro_state.total_catalyst_fees);
        let drift = total_in as i128 - total_out as i128;
        GlobalSolvencyView {
            total_minted,
            total_burned: self.macro_state.total_snacc_burned,
            sum_account_balances: self.macro_state.sum_account_balances,
            sum_strain_snacc: self.macro_state.sum_strain_snacc,
            sum_treasury: self.treasury,
            sum_dividend_pool: self.macro_state.industrial_dividend_pool,
            epoch_dividend_inflow: self.macro_state.epoch_dividend_inflow,
            sum_npc_reserve: self.macro_state.sum_npc_reserve,
            total_catalyst_fees: self.macro_state.total_catalyst_fees,
            drift,
            solvent: drift == 0,
        }
    }
}

/// J3.9 — tampilan invarian `V-04` (semua komponen + drift bertanda).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalSolvencyView {
    pub total_minted: u128,
    pub total_burned: u128,
    pub sum_account_balances: u128,
    pub sum_strain_snacc: u128,
    pub sum_treasury: u128,
    pub sum_dividend_pool: u128,
    pub epoch_dividend_inflow: u128,
    pub sum_npc_reserve: u128,
    pub total_catalyst_fees: u128,
    /// `pemasukan − pengeluaran`; `0` = solvent.
    pub drift: i128,
    pub solvent: bool,
}

#[cfg(test)]
mod qa_en117_tests;

#[cfg(test)]
mod qa_organel_tests;

#[cfg(test)]
mod qa_en104_tests;

#[cfg(test)]
mod qa_ma_tests;

#[cfg(test)]
mod qa_dk026_tests;

#[cfg(test)]
mod qa_audit_tests;

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
            .current_account_id("snacc-v001.testnet".parse().unwrap())
            .predecessor_account_id(predecessor.parse().unwrap())
            .attached_deposit(deposit)
            .block_timestamp(block_timestamp);
        testing_env!(builder.build());
    }

    /// Pipeline #7 — suffix sub-akun diturunkan dari akun kontrak (regresi bug
    /// `VAULT_SUBACCOUNT_SUFFIX` hardcode `.snacc-v001.near` yang gagal di testnet).
    #[test]
    fn pipeline7_suffix_mengikuti_akun_kontrak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(
            c.get_strain(1).vault_subaccount.to_string(),
            "s-1-0.snacc-v001.testnet"
        );
        // Kontrak dengan akun lain -> suffix mengikuti (parent selalu ada).
        let mut b = VMContextBuilder::new();
        b.current_account_id("snacc-lab.testnet".parse().unwrap())
            .predecessor_account_id(ALKEMIS_ID.parse().unwrap())
            .attached_deposit(NearToken::from_yoctonear(10_000))
            .block_timestamp(3_600_000_000_000);
        testing_env!(b.build());
        let v = c.incubate_strain(state::StrainRole::Extractor, U128(10_000), 0);
        assert_eq!(v.vault_subaccount.to_string(), "s-2-0.snacc-lab.testnet");
    }

    /// Pipeline #7 — `feed_strain` kini payable: deposit menempel TIDAK memicu trap;
    /// dipanggil dengan deposit -> sukses (refund via promise).
    #[test]
    fn pipeline7_feed_payable_dengan_deposit() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000_000)); // deposit menempel
        let v = c.feed_strain(1, U128(1_000)); // dulu: trap non-payable
        assert_eq!(v.efficiency_bps, 10000);
    }

    /// Pipeline #7 — `claim_industrial_dividend` kini payable (regresi yang sama).
    #[test]
    fn pipeline7_claim_payable_dengan_deposit() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(500_000)); // deposit menempel
        c.synthesize_and_redeem_aether(2, U128(1));
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
        c.advance_epoch();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000_000)); // deposit menempel
        let payout = c.claim_industrial_dividend(2, 0); // dulu: trap non-payable
        assert!(payout.0 > 0);
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
        // Pipeline #7: suffix = akun kontrak (`.testnet` di testnet).
        assert_eq!(v1.vault_subaccount.to_string(), "s-1-0.snacc-v001.testnet");
        let v2 = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        assert_eq!(v2.vault_subaccount.to_string(), "s-2-0.snacc-v001.testnet");
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
        assert_eq!(view.vault_subaccount.to_string(), "s-1-0.snacc-v001.testnet");
        assert_eq!(view.vitality, 8500);
        assert_eq!(view.efficiency_bps, 10000);
        assert_eq!(view.bmr_per_hour, 100);
        assert_eq!(view.state_nonce, 0);
        assert_eq!(view.pending_callbacks, 0);
        assert_eq!(view.atp_balance, 50); // `INITIAL_ATP` (§13a)
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

        // Titik waktu 1: t1 = 3600s, fee = 10_000 + kaki K3 (1 spora × 5 × 100 = 500)
        // -> volume_epoch = 10_500; temp = floor(1000×10_500/10_000) = 1050 > 610 (Fever!)
        // -> burn = 350 bps × 10_000 = 350 -> supply = 9_650.
        let t1_ns = 3_600_000_000_000u64;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(10_000), t1_ns);
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.total_snacc_supply, 9_650);
        assert_eq!(c.get_crucible_temp(), 1088); // floor(1000×10_500/9_650)

        // Titik waktu 2: t2 = t1 + 40 detik -> decay = 2_000 -> V_decayed = 8_500
        // temp(t2) = floor(1000 × 8_500 / 9_650) = 880 mK (masih Fever > 610)
        let t2_ns = t1_ns + 40 * 1_000_000_000;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(0), t2_ns);
        assert_eq!(c.get_crucible_temp(), 880);

        // Setelah 500 detik (decay 25_000 > 10_500) -> saturasi ke 0
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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

        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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

        // Zona Normal baru = [600, 610] (ECON-111: Fever >610, Hypo <600).
        // Catatan urutan: termostat mengevaluasi suhu dari fee SEBELUM kaki K2/K3
        // ditambahkan; kaki terlihat oleh evaluasi berikutnya (lazy).
        // fee 6.100 -> temp inside = 610 -> NORMAL (tepat di batas atas): tanpa burn/rebate.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(6_100));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(6_100));
        // volume setelah kaki = 6.600 -> view temp = 660.
        assert_eq!(c.get_crucible_temp(), 660);
        assert_eq!(c.macro_state.total_snacc_burned, 0);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000);
        // `DK-033`: fee 6.100 dibagi 50/50 — treasury 2.000+3.050, inflow 3.050.
        assert_eq!(c.treasury, 5_050);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 3_050);

        // Satu langkah di atas batas: fee 100 -> temp inside 670 > 610 -> FEVER.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(100));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(100));
        // burn 350 bps × 100 = 3 -> supply 9.997; volume 6.700+500 = 7.200
        // -> view temp = floor(1000×7.200/9.997) = 720.
        assert_eq!(c.get_crucible_temp(), 720);
        assert_eq!(c.macro_state.total_snacc_burned, 3);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000);
        // remainder 97: inflow 48, treasury +49.
        assert_eq!(c.treasury, 5_099);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 3_098);
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
        // ECON-111: fee 9.000 + kaki 500 -> volume 9.500; temp 950 > 610 (Fever)
        // -> burn 350 bps × 9.000 = 315 -> supply 9.685; temp = floor(1000×9.500/9.685) = 980.
        assert_eq!(temp, 980);
        assert_eq!(view.crucible_temp, temp);
        assert_eq!(view.total_snacc_supply, 9_685);
        assert_eq!(view.volume_epoch, 9_500);

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
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0)); // SEC-113: owner saat deploy
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
            c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
        c.advance_epoch();
        // ECON-111: extract ke-2 masuk Fever (temp 683 > 610) -> burn 350 ->
        // inflow = 5.000 + 4.825 = 9.825; rate = 9.825×1e18/4 = 2.45625e21.
        let pool = c.get_dividend_pool().0 .0;
        assert_eq!(pool, 9_825);
        let p2 = c.claim_industrial_dividend(2, 0).0;
        let p3 = c.claim_industrial_dividend(3, 0).0;
        assert_eq!(p2, 2_456); // 1 × rate / 1e18
        assert_eq!(p3, 7_368); // 3 × rate / 1e18
        // `TC-DIV-05`: Σ payout ≤ pool; sisa pembulatan 1 tetap di kolam.
        assert!(p2 + p3 <= pool);
        assert_eq!(c.get_dividend_pool().0 .0, 1);
    }

    /// Kolam kosong (rate 0) -> `ERR_DIVIDEND_POOL_EMPTY`.
    #[test]
    #[should_panic(expected = "ERR_DIVIDEND_POOL_EMPTY")]
    fn dk033_kolam_kosong_ditolak() {
        let mut c = dk_setup();
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
        c.advance_epoch();
        // ECON-111: inflow = 5.000 + 4.825 (Fever burn 350) = 9.825; rate = 2.45625e21;
        // kontribusi 3:1 -> payout 7.368 : 2.456 (pro-rata, bukan first-mover).
        let p3 = c.claim_industrial_dividend(3, 0).0; // urutan klaim dibalik
        let p2 = c.claim_industrial_dividend(2, 0).0;
        assert_eq!(p3, 2_456);
        assert_eq!(p2, 7_368);
        assert_eq!(p2 + p3, 9_824);
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
        c.advance_epoch();
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""verdict":"OVERFED""#));
    }

    /// `EN-106` — event `zone_changed {from_zone, to_zone, crucible_temp}` saat zona berganti.
    #[test]
    fn en106_zone_changed_event() {
        let mut c = dk_setup(); // supply 20_000
        // Extract fee 10_000 -> temp inside = floor(1000×10.000/20.000) = 500.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"zone_changed""#));
        assert!(logs.contains(r#""from_zone":"NORMAL""#));
        assert!(logs.contains(r#""to_zone":"HYPO""#));
        // temp inside = floor(1000×10.000/20.000) = 500 (<600 -> HYPO; kaki K2/K3
        // ditambahkan SETELAH evaluasi suhu, terlihat oleh evaluasi berikutnya).
        assert!(logs.contains(r#""crucible_temp":500"#));
        // Extract lagi -> volume 21.000 -> temp 1025 > 610 -> HYPO->FEVER.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""from_zone":"HYPO""#));
        assert!(logs.contains(r#""to_zone":"FEVER""#));
        assert!(logs.contains(r#""crucible_temp":1025"#));
        assert_eq!(c.macro_state.last_zone, state::MacroZone::Fever);
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
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0)); // SEC-113: owner saat deploy
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
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
        c.advance_epoch();
        let payout = c.claim_industrial_dividend(2, 0).0;
        assert!(payout > 0);
        // Meski dividen diklaim penuh, total masih rugi bersih (`DK-030`).
        let bal2 = c.snacc_balances.get(&1).copied().unwrap();
        assert!(bal2 + payout < bal0, "rebate+dividen tidak boleh melampaui fee");
    }

    // ------------------------------------------------------------------
    // Pipeline #3 — EN-120 (organel merger), EN-121 (koloni), EN-122 (divide),
    // EN-123 (adopt), EN-125 (risk heat), NPC ADR-30
    // ------------------------------------------------------------------

    /// `EN-120` — merger menyerap organel (max per slot) + reset `last_allocate_epoch`.
    #[test]
    fn en120_splice_organel_max_dan_reset_alokasi() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c); // strain 1 (Extractor) & 2 (Fermenter), milik sama
        // Organel: dominan mito=2, terserap mito=3 -> 3; nukleus dom=0, abs=1 -> 1.
        let mut dom = c.strains.get(&1).unwrap().clone();
        dom.organel.mitokondria = 2;
        dom.last_allocate_epoch = 7;
        let _ = c.strains.insert(1, dom);
        let mut abs = c.strains.get(&2).unwrap().clone();
        abs.organel.mitokondria = 3;
        abs.organel.nukleus = 1;
        let _ = c.strains.insert(2, abs);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.splice_strains(1, 2);
        assert_eq!(v.organel.mitokondria, 3); // max(2,3)
        assert_eq!(v.organel.nukleus, 1); // max(0,1)
        assert_eq!(v.last_allocate_epoch, u64::MAX); // realokasi ATP dibuka kembali
    }

    /// `EN-120` — hostile takeover juga mewarisi organel + reset alokasi.
    #[test]
    fn en120_hostile_organel_max_dan_reset_alokasi() {
        let mut c = hostile_setup();
        let mut t = c.strains.get(&1).unwrap().clone();
        t.organel.lisosom = 4;
        t.organel.nukleus = 1;
        t.last_allocate_epoch = 5;
        let _ = c.strains.insert(1, t);
        let mut r = c.strains.get(&2).unwrap().clone();
        r.organel.lisosom = 2;
        r.organel.nukleus = 3;
        let _ = c.strains.insert(2, r);
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 117 * 3_600_000_000_000,
        );
        let (_, view) = c.hostile_takeover_splice(2, 1, 0);
        assert_eq!(view.organel.lisosom, 4); // max(2,4)
        assert_eq!(view.organel.nukleus, 3); // max(3,1)
        assert_eq!(view.last_allocate_epoch, u64::MAX);
    }

    /// `EN-121` — `get_colony`: sel hidup milik akun; sel mati tidak masuk koloni.
    #[test]
    fn en121_get_colony_filter_sel_hidup() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, SYNTHESIZER_ROLE, 0);
        let colony = c.get_colony(ALKEMIS_ID.parse().unwrap());
        assert_eq!(colony.len(), 3);
        assert!(colony.iter().all(|v| v.owner_id == ALKEMIS_ID));
        // Likuidasi satu sel -> keluar dari koloni.
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        assert_eq!(c.get_colony(ALKEMIS_ID.parse().unwrap()).len(), 2);
        assert_eq!(c.colony_active_count(&ALKEMIS_ID.parse().unwrap()), 2);
        // Koloni akun lain kosong.
        assert_eq!(c.get_colony(RAIDER.parse().unwrap()).len(), 0);
    }

    /// `EN-122` — sitokinesis: Σ saldo induk+anak = saldo induk; vitality & spora separuh;
    /// organel ÷2; gen anak = induk+1; event `strain_divided`.
    #[test]
    fn en122_divide_sitokinesis_konservasi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8_000, vitality 8500
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 7);
        let mut p = c.strains.get(&1).unwrap().clone();
        p.organel = state::OrganelleSlots { mitokondria: 3, nukleus: 1, lisosom: 0 };
        let _ = c.strains.insert(1, p);
        // `EN-124`: divide butuh anggaran demografi (merges+lyses >= divides) —
        // inkubasi satu sel korban lalu likuidasi (lyses = 1) sebagai anggaran.
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 2
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let (parent, child) = c.divide_strain(1);
        // Σ saldo = 8_000 (`I-01`); vitality & spora separuh.
        assert_eq!(parent.vitality, 4250);
        assert_eq!(child.vitality, 4250);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 4_000);
        // Anak = strain 3 (strain 2 = korban lisis untuk anggaran demografi EN-124).
        assert_eq!(c.snacc_balances.get(&3).copied().unwrap(), 4_000);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&4));
        assert_eq!(c.inventories.get(&(3, "mt:spore-ignis".to_string())), Some(&3));
        // Organel ÷2: anak floor(3/2)=1 mito, induk sisa 2; nukleus anak 0, induk 1.
        assert_eq!(child.organel.mitokondria, 1);
        assert_eq!(parent.organel.mitokondria, 2);
        assert_eq!(child.organel.nukleus, 0);
        assert_eq!(parent.organel.nukleus, 1);
        // Genom diwariskan; gen anak = induk+1; koloni bertambah.
        assert_eq!(child.generation, 1);
        assert_eq!(child.role_mask, state::constants::ROLE_EXTRACTOR);
        assert_eq!(child.efficiency_bps, 10000);
        assert_eq!(child.owner_id.to_string(), ALKEMIS_ID);
        assert_eq!(child.vault_subaccount.to_string(), "s-3-0.snacc-v001.testnet");
        assert_eq!(child.strain_id, 3);
        assert_eq!(c.get_colony(ALKEMIS_ID.parse().unwrap()).len(), 2); // induk + anak
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"strain_divided""#));
        assert!(logs.contains(r#""child_balance":4000"#));
        // `D8` log hanya dari divide (harvest F6 tidak menulis MnaLog — lihat report).
        assert_eq!(c.mna_logs.len(), 1);
    }

    /// `EN-122` — pagar koloni penuh (`MAX_COLONY_SIZE = 5`).
    #[test]
    #[should_panic(expected = "ERR_COLONY_FULL")]
    fn en122_koloni_penuh_ditolak() {
        let mut c = SnaccContract::new();
        for _ in 0..5 {
            let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        }
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.divide_strain(1); // koloni sudah 5
    }

    /// `EN-122` — pagar generasi (`MAX_GENERATION = 8`).
    #[test]
    #[should_panic(expected = "ERR_MAX_GENERATION")]
    fn en122_max_generation_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let mut p = c.strains.get(&1).unwrap().clone();
        p.generation = 8;
        let _ = c.strains.insert(1, p);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.divide_strain(1); // anak gen 9 > 8
    }

    /// `EN-123` — adopt sukses: bayar book value TANPA premium ke pemilik lama,
    /// ownership pindah, selamat dari lisis, event `strain_adopted`.
    #[test]
    fn en123_adopt_sukses_ownership_pindah() {
        let mut c = hostile_setup(); // target distress eff 1500; raider milik RAIDER (bal 800_000)
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 2); // book = 8_002
        let old_owner_bal = c.account_balance(&ALKEMIS_ID.parse().unwrap());
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let v = c.adopt(1, 2, 0);
        // Ownership pindah ke pemilik raider; target selamat (status tetap hidup).
        assert_eq!(v.owner_id.to_string(), RAIDER);
        assert_eq!(c.get_strain(1).owner_id.to_string(), RAIDER);
        // Status tersimpan tetap hidup (Active); view menurunkan ke Distressed
        // karena vitalitas efektif masih Tier 2 (audit W10).
        assert_eq!(c.strains.get(&1).unwrap().status, StrainStatus::Active);
        assert_eq!(c.get_strain(1).status, StrainStatus::Distressed);
        // Pembayaran: 100% book value ke pemilik lama (tanpa premium, tanpa potongan).
        assert_eq!(
            c.account_balance(&ALKEMIS_ID.parse().unwrap()),
            old_owner_bal + 8_002
        );
        assert_eq!(c.snacc_balances.get(&2).copied().unwrap(), 800_000 - 8_002);
        // Indeks koloni ter-update di dua sisi.
        assert_eq!(c.colony_active_count(&ALKEMIS_ID.parse().unwrap()), 0);
        assert_eq!(c.colony_active_count(&RAIDER.parse().unwrap()), 2);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"strain_adopted""#));
        assert!(logs.contains(r#""book_value":8002"#));
        assert_eq!(c.mna_logs.len(), 1); // kind Adopt
    }

    /// `EN-123` — target sehat (eff > 2000) tidak bisa diadopsi.
    #[test]
    #[should_panic(expected = "ERR_NOT_DISTRESSED")]
    fn en123_adopt_bukan_distressed_ditolak() {
        let mut c = hostile_setup();
        set_context_at(RAIDER, NearToken::from_yoctonear(0), 3_600_000_000_000);
        let _ = c.adopt(1, 2, 0);
    }

    /// `EN-123` — koloni pengadopsi penuh -> `ERR_COLONY_FULL`.
    #[test]
    #[should_panic(expected = "ERR_COLONY_FULL")]
    fn en123_adopt_koloni_penuh_ditolak() {
        let mut c = hostile_setup();
        for _ in 0..4 {
            set_context(RAIDER, NearToken::from_yoctonear(10_000));
            let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(10_000), 0);
        }
        // RAIDER kini punya 5 sel aktif -> adopt ke-6 ditolak.
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.adopt(1, 2, 0);
    }

    /// `EN-123` — target sudah Liquidated -> `ERR_ALREADY_LIQUIDATED`.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn en123_adopt_target_liquidated_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(1_000_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(1_000_000), 0);
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        set_context(RAIDER, NearToken::from_yoctonear(0));
        let _ = c.adopt(1, 2, 0);
    }

    /// `EN-125` — `get_risk_heat`: dasar proksimitas Fever + bonus burst/dividen; cap 10000.
    #[test]
    fn en125_risk_heat_komponen_dan_cap() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        // Fresh: temp 0, tanpa burst, pool 0 -> heat 0.
        assert_eq!(c.get_risk_heat(), 0);
        // Komponen 1: suhu naik -> heat = temp × 10000 / fever_threshold(610, ECON-111).
        c.macro_state.volume_epoch = 300_000_000_000u128; // temp = 1000×3e11/1e12 = 300
        c.macro_state.last_update_ms = 3_600_000;
        let expected = 300 * 10_000 / 610; // 4918
        assert_eq!(c.get_risk_heat(), expected);
        // Komponen 2: burst aktif +1000.
        c.macro_state.nutrient_burst_bps = 2_000;
        c.macro_state.nutrient_burst_until = 1;
        assert_eq!(c.get_risk_heat(), expected + 1_000);
        // Komponen 3: kolam dividen ≥ 1% supply +1000.
        c.macro_state.industrial_dividend_pool = c.macro_state.total_snacc_supply / 100;
        assert_eq!(c.get_risk_heat(), expected + 2_000);
        // Cap 10000: temp ekstrem.
        c.macro_state.volume_epoch = u128::MAX / 4;
        assert_eq!(c.get_risk_heat(), 10_000);
        // Surfacing di appetite meter.
        c.macro_state.volume_epoch = 0;
        c.macro_state.nutrient_burst_bps = 0;
        c.macro_state.industrial_dividend_pool = 0;
        let m = c.get_appetite_meter();
        assert_eq!(m.risk_heat, 0);
    }

    /// `ADR-30` — 3 NPC ter-seed dengan profil `D4`.
    #[test]
    fn npc_tiga_terseed() {
        let c = SnaccContract::new();
        let e = c.get_npc("npc-extractor".to_string());
        assert_eq!(e.role_archetype, "Pyre-X");
        assert_eq!(e.wallet_account, "npc-extractor.snacc-v001.near");
        assert_eq!(e.spores_stock, 500);
        assert_eq!(e.target_spores_stock, 500);
        assert_eq!(e.base_spore_price, 100);
        assert_eq!(c.get_npc("npc-central-arb".to_string()).spores_stock, 1000);
        assert_eq!(c.get_npc("npc-vulture-ma".to_string()).spores_stock, 300);
    }

    #[test]
    #[should_panic(expected = "ERR_NPC_UNKNOWN")]
    fn npc_tak_dikenal_ditolak() {
        let c = SnaccContract::new();
        let _ = c.get_npc("npc-hantu".to_string());
    }

    /// `ADR-30`/`I-15` — kuotasi: harga unit = floor(P_base × S_target / S_after), naik saat stok menyusut.
    #[test]
    fn npc_quote_formula_kelangkaan() {
        let c = SnaccContract::new();
        // extractor 500/500: beli 100 -> S_after 400 -> unit = floor(100×500/400) = 125.
        let q = c.npc_quote_spores("npc-extractor".to_string(), 100);
        assert_eq!(q.unit_price_vita, 125);
        assert_eq!(q.total_cost_snacc, 12_500);
        assert_eq!(q.npc_spores_stock_after, 400);
        assert!(q.reasoning_tooltip.contains("Scarcity"));
        // Beli kecil: harga tetap P_base saat S_after ≥ dekat target (floor pembulatan).
        let q2 = c.npc_quote_spores("npc-vulture-ma".to_string(), 1);
        assert_eq!(q2.unit_price_vita, 100); // floor(100×300/299) = 100
        // Harga naik makin deplet: beli 299 dari vulture -> S_after 1 -> unit = 30.000.
        let q3 = c.npc_quote_spores("npc-vulture-ma".to_string(), 299);
        assert_eq!(q3.unit_price_vita, 30_000);
    }

    /// `I-15` — konservasi persis: Δreserve == −Δsaldo == total_cost; Δstok == Δinventori.
    #[test]
    fn npc_buy_konservasi_i15() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // buy 40 -> S_after 460 -> unit = floor(50_000/460) = 108 -> total 4.320.
        let q = c.npc_buy_spores(1, "npc-extractor".to_string(), 40);
        assert_eq!(q.unit_price_vita, 108);
        assert_eq!(q.total_cost_snacc, 4_320);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000 - 4_320);
        let npc = c.get_npc("npc-extractor".to_string());
        assert_eq!(npc.snacc_reserve, 4_320); // == pengurangan saldo (I-15)
        assert_eq!(npc.spores_stock, 460); // == penambahan inventori (I-15)
        assert_eq!(npc.trades_executed, 1);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&40));
        assert_eq!(c.get_strain(1).state_nonce, 1); // `ADR-29`
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"npc_spores_bought""#));
    }

    #[test]
    #[should_panic(expected = "ERR_NPC_OUT_OF_STOCK")]
    fn npc_stok_kurang_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 501);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn npc_buy_non_owner_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(0));
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 1);
    }

    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn npc_buy_saldo_kurang_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // buy 100 -> total 12.500 > 8.000.
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 100);
    }
    // ------------------------------------------------------------------
    // Pipeline #4 — EN-114..119 (ATP/mitokondria) + kalibrasi ECON-111
    // ------------------------------------------------------------------

    /// `EN-115`/§13a — rumus `calc_atp_minted`: base + bonus mitokondria +10%/level.
    #[test]
    fn atp115_rumus_dan_bonus_mitokondria() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // fee 10 × 10^15 -> base 10; eff 10000 -> 10.
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 0), 10);
        // Mitokondria +10%/level: L1 -> 11, L2 -> 12, L5 -> 15.
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 1), 11);
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 2), 12);
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 5), 15);
        // efficiency 5000 -> setengah; fee di bawah 1 unit -> 0.
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 5_000, 0), 5);
        assert_eq!(SnaccContract::calc_atp_minted(999, 10_000, 5), 0);
    }

    /// `EN-114` — inkubasi memberi `INITIAL_ATP = 50` (energi kelahiran).
    #[test]
    fn atp114_incubasi_init_50() {
        let mut c = SnaccContract::new();
        let v = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(v.atp_balance, constants::INITIAL_ATP); // 50
        assert_eq!(v.last_allocate_epoch, u64::MAX); // belum pernah alokasi
    }

    /// `EN-06`/§13a — feed mencetak ATP (fee besar ≥ 10^15) + tampil di event `fed`.
    #[test]
    fn atp114_feed_mint_atp_dan_event() {
        let mut c = SnaccContract::new();
        // stake 1e16 -> saldo strain 8e15 = 8 unit ATP-yocto (cukup untuk 2 feed).
        let _ = call_incubate(&mut c, 10_000_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        c.macro_state.params.dormancy_threshold_bps = 0;
        // vitality penuh -> rescue multiplier 10000 -> effective = amount.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.vitality = constants::VITALITY_CAP;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(2 * constants::ATP_YOCTO_UNIT));
        // mitokondria 0 -> atp_minted = 2.
        assert_eq!(c.get_strain(1).atp_balance, 52);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""atp_minted":2"#), "atp_minted hilang dari event fed");
        // Bonus mitokondria: set L2 -> feed 5 unit -> minted = floor(5×1.2) = 6.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.organel.mitokondria = 2;
        let _ = c.strains.insert(1, v);
        c.feed_strain(1, U128(5 * constants::ATP_YOCTO_UNIT));
        assert_eq!(c.get_strain(1).atp_balance, 52 + 6);
    }

    /// `EN-116`/§13a — konsumsi ATP extract = 1; habis -> `ERR_NOT_ENOUGH_ATP`.
    #[test]
    #[should_panic(expected = "ERR_NOT_ENOUGH_ATP")]
    fn atp116_konsumsi_extract_dan_habis_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
        assert_eq!(c.get_strain(1).atp_balance, 49);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""purpose":"extract","cost":1"#));
        // Drain ATP -> extract berikutnya ditolak.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.atp_balance = 0;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_ENOUGH_ATP")]
    fn atp116_transmute_tanpa_atp_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 2);
        let mut v = c.strains.get(&1).unwrap().clone();
        v.atp_balance = 1; // transmute butuh 2
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
    }

    /// `EN-116`/§13a — splice 5 & hostile 10.
    #[test]
    fn atp116_biaya_splice_dan_hostile() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 2);
        assert_eq!(c.get_strain(1).atp_balance, 45); // 50 - 5
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""purpose":"splice","cost":5"#));
        let mut c2 = hostile_setup();
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c2.hostile_takeover_splice(2, 1, 0);
        assert_eq!(c2.get_strain(2).atp_balance, 40); // 50 - 10
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""purpose":"hostile","cost":10"#));
    }

    /// `EN-118`/§13a — `allocate_atp`: biaya 1/kenaikan level, event `atp_allocated`.
    #[test]
    fn atp118_allocate_siklus_lengkap() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // atp 50
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.allocate_atp(1, 2, 1, 1);
        assert_eq!(v.organel.mitokondria, 2);
        assert_eq!(v.organel.nukleus, 1);
        assert_eq!(v.organel.lisosom, 1);
        assert_eq!(v.atp_balance, 46); // biaya 2+1+1 = 4
        assert_eq!(v.last_allocate_epoch, 0);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"atp_allocated""#));
        assert!(logs.contains(r#""cost":4"#));
    }

    #[test]
    #[should_panic(expected = "ERR_ALREADY_ALLOCATED")]
    fn atp118_allocate_dua_kali_seepoch_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 1, 0, 0);
        let _ = c.allocate_atp(1, 2, 0, 0);
    }

    #[test]
    #[should_panic(expected = "ERR_ORGANEL_LEVEL_MAX")]
    fn atp118_level_enam_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 6, 0, 0);
    }

    /// `EN-118` — epoch baru membuka alokasi; biaya hanya KENAIKAN level.
    #[test]
    fn atp118_epoch_baru_biaya_kenaikan_saja() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 2, 1, 1); // biaya 4 -> 46
        c.macro_state.epoch_ends_sec = 0; // tes: lewati gerbang waktu W6
        c.advance_epoch();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.allocate_atp(1, 3, 1, 1); // kenaikan mito 1 -> biaya 1 -> 45
        assert_eq!(v.atp_balance, 45);
        assert_eq!(v.last_allocate_epoch, 1);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn atp118_allocate_non_owner_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 1, 0, 0);
    }

    /// `EN-119` — nukleus & lisosom: TIDAK ada efek ekonomi dikarang; tersimpan & tampil.
    #[test]
    fn en119_nukleus_lisosom_tampil_tanpa_efek_ekonomi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 1, 4, 5);
        let v = c.get_strain(1);
        assert_eq!(v.organel.nukleus, 4);
        assert_eq!(v.organel.lisosom, 5);
        // Satu-satunya efek ekonomi = mitokondria (rumus ATP §13a).
        assert_eq!(
            SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 1),
            11
        );
    }

    /// `ECON-111` — kaki K2/K3/K4 masuk `volume_epoch`; eksklusi wajib tidak masuk.
    #[test]
    fn econ111_volume_multikaki_dan_eksklusi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8.000
        // K3: extract 1 spora -> +1×5×100 = 500.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
        assert_eq!(c.macro_state.volume_epoch, 500);
        // K2+K3: transmute 2 spora -> burn 2, mint 2 -> +2.000 -> total 2.500.
        // (extractor diberi bit Fermenter via merger manual untuk keperluan test)
        grant(&mut c, "mt:spore-ignis", 2); // stok cukup untuk 1 batch (2 spora)
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011;
        let _ = c.strains.insert(1, v);
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert_eq!(c.macro_state.volume_epoch, 2_500);
        // EKSKLUSI: inkubasi modal TIDAK menambah volume.
        set_context(RAIDER, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(0), 0);
        assert_eq!(c.macro_state.volume_epoch, 2_500);
        // EKSKLUSI: ft_transfer TIDAK menambah volume.
        fund(&mut c, ALKEMIS_ID, 100);
        register(&mut c, ALKEMIS_ID);
        register(&mut c, RAIDER);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(RAIDER.parse().unwrap(), U128(50), None);
        assert_eq!(c.macro_state.volume_epoch, 2_500);
        // K4: likuidasi strain 2 (insolven) -> volume += bounty+recovery = saldo penuh.
        let bal2 = c.snacc_balances.get(&2).copied().unwrap();
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(2);
        assert_eq!(c.macro_state.volume_epoch, 2_500 + bal2);
    }
    // ------------------------------------------------------------------
    // Pipeline #5 — LC-16 (nft_transfer), LC-20 (konjugasi), LC-11 (diskon burn),
    // EN-124 (invariant demografi), SEC-110 (nonce adopt), get_colony_atp
    // ------------------------------------------------------------------

    /// LC-16 — transplantasi sukses: sel utuh pindah (saldo, spora, organel, ATP),
    /// indeks koloni dua sisi ter-update, event standard `nep171`.
    #[test]
    fn lc16_nft_transfer_sukses() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8.000, atp 50
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.nft_transfer(
            ALICE.parse().unwrap(),
            "1".to_string(),
            None,
            Some("transplantasi".to_string()),
        );
        let v = c.get_strain(1);
        assert_eq!(v.owner_id.to_string(), ALICE);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000); // perut ikut
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&3));
        assert_eq!(v.atp_balance, 50); // ATP ikut vessel
        // Indeks dua sisi.
        assert_eq!(c.get_colony(ALKEMIS_ID.parse().unwrap()).len(), 0);
        assert_eq!(c.get_colony(ALICE.parse().unwrap()).len(), 1);
        assert_eq!(c.colony_active_count(&ALICE.parse().unwrap()), 1);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""standard":"nep171""#));
        assert!(logs.contains(r#""event":"nft_transfer""#));
        assert!(logs.contains(r#""old_owner_id":"alkemis.testnet""#));
        assert!(logs.contains(r#""new_owner_id":"alice.testnet""#));
        assert!(logs.contains(r#""token_ids":["1"]"#));
    }

    #[test]
    #[should_panic(expected = "ERR_YOCTO_REQUIRED")]
    fn lc16_nft_transfer_tanpa_yocto_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.nft_transfer(ALICE.parse().unwrap(), "1".to_string(), None, None);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn lc16_nft_transfer_bukan_pemilik_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(1));
        c.nft_transfer(ALICE.parse().unwrap(), "1".to_string(), None, None);
    }

    /// LC-16 — sel mati tidak dapat dipindah (keputusan tercatat).
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn lc16_nft_transfer_sel_mati_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.nft_transfer(ALICE.parse().unwrap(), "1".to_string(), None, None);
    }

    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn lc16_nft_transfer_token_tak_ada_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.nft_transfer(ALICE.parse().unwrap(), "99".to_string(), None, None);
    }

    /// LC-16 — `nft_transfer_call`: penerima menerima (resolve `false`).
    #[test]
    fn lc16_transfer_call_diterima() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.nft_transfer_call(
            ALICE.parse().unwrap(),
            "1".to_string(),
            "jual".to_string(),
            None,
            None,
        );
        // Simulasikan callback: kontrak penerima MENERIMA (false).
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
                near_sdk::serde_json::to_vec(&false).unwrap()
            )]
        );
        c.nft_resolve_transfer(
            ALKEMIS_ID.parse().unwrap(),
            ALICE.parse().unwrap(),
            "1".to_string(),
        );
        assert_eq!(c.get_strain(1).owner_id.to_string(), ALICE);
    }

    /// LC-16 — `nft_transfer_call`: promise gagal -> token kembali ke pemilik lama.
    #[test]
    fn lc16_transfer_call_gagal_kembali() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.nft_transfer_call(
            ALICE.parse().unwrap(),
            "1".to_string(),
            "jual".to_string(),
            None,
            None,
        );
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
        c.nft_resolve_transfer(
            ALKEMIS_ID.parse().unwrap(),
            ALICE.parse().unwrap(),
            "1".to_string(),
        );
        // Kembali ke pemilik lama + indeks pulih.
        assert_eq!(c.get_strain(1).owner_id.to_string(), ALKEMIS_ID);
        assert_eq!(c.get_colony(ALKEMIS_ID.parse().unwrap()).len(), 1);
        assert_eq!(c.get_colony(ALICE.parse().unwrap()).len(), 0);
    }

    /// LC-20 — konjugasi: trait disalin (sumber tetap), biaya ATP target 2.
    #[test]
    fn lc20_conjugate_sukses() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 2, pemilik sama
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // Sumber (1) pernah sukses mutasi TEE (trait ada); target (2) belum.
        let mut f = c.strains.get(&1).unwrap().clone();
        f.phenotype_trait = Some("Thermo-Aureate".to_string());
        let _ = c.strains.insert(1, f);
        let v = c.conjugate_traits(
            1,
            2,
            "Thermo-Aureate".to_string(),
        );
        assert_eq!(v.phenotype_trait.as_deref(), Some("Thermo-Aureate"));
        assert_eq!(v.atp_balance, 48); // 50 - 2 (dibayar target)
        // Sumber tetap memiliki traitnya (salin, bukan pindah).
        assert_eq!(
            c.get_strain(1).phenotype_trait.as_deref(),
            Some("Thermo-Aureate")
        );
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"trait_conjugated""#));
    }

    #[test]
    #[should_panic(expected = "ERR_TRAIT_MISMATCH")]
    fn lc20_conjugate_trait_mismatch_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 2, pemilik sama
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.conjugate_traits(1, 2, "Trait-Ngawur".to_string());
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_SAME_OWNER")]
    fn lc20_conjugate_beda_pemilik_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 1 = ALKEMIS
        set_context(RAIDER, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(10_000), 0); // strain 2 = RAIDER
        set_context(RAIDER, NearToken::from_yoctonear(0));
        // caller = RAIDER pemilik sumber (2), tapi target (1) milik ALKEMIS.
        let mut f = c.strains.get(&2).unwrap().clone();
        f.phenotype_trait = Some("Thermo-Aureate".to_string());
        let _ = c.strains.insert(2, f);
        let _ = c.conjugate_traits(2, 1, "Thermo-Aureate".to_string());
    }

    /// LC-11 — pemegang `mt:elixir-homeostasis` mendapat diskon 50% burn Fever.
    #[test]
    fn lc11_homeostasis_diskon_burn_pemegang() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // supply 10.000
        let _ = c.inventories.insert((1, "mt:elixir-homeostasis".to_string()), 1);
        let supply_before = c.macro_state.total_snacc_supply;
        // fee 20.000 -> temp 2000 > 610 (Fever); burn pemegang = 20.000×350×50/(100×10.000) = 350.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(20_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(20_000));
        let burn = supply_before - c.macro_state.total_snacc_supply;
        assert_eq!(burn, 350); // setengah dari 700
        // Eliksir tidak dikonsumsi (efek pasif).
        assert_eq!(
            c.inventories.get(&(1, "mt:elixir-homeostasis".to_string())),
            Some(&1)
        );
    }

    #[test]
    fn lc11_bukan_pemegang_burn_penuh() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let supply_before = c.macro_state.total_snacc_supply;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(20_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(20_000));
        let burn = supply_before - c.macro_state.total_snacc_supply;
        assert_eq!(burn, 700); // penuh 350 bps
    }

    /// EN-124 — gate demografi: divide tanpa anggaran (merges+lyses < divides) ditolak.
    #[test]
    #[should_panic(expected = "ERR_BIRTHS_EXCEEDED")]
    fn en124_gate_tanpa_anggaran_ditolak() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(c.macro_state.total_births, 2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.divide_strain(1); // divides 1 > merges 0 + lyses 0
    }

    /// EN-124 — lisis membuka anggaran divide; counter konsisten.
    #[test]
    fn en124_gate_dan_counter() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(c.macro_state.total_births, 2);
        // Likuidasi satu sel -> lyses = 1 -> anggaran divide terbuka.
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(2);
        assert_eq!(c.macro_state.total_lyses, 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.divide_strain(1);
        assert_eq!(c.macro_state.total_divides, 1);
        assert_eq!(c.macro_state.total_births, 3); // 2 inkubasi + 1 divide
        // Invariant: divides (1) <= mergers (0) + lyses (1).
        assert!(c.macro_state.total_divides
            <= c.macro_state.total_mergers + c.macro_state.total_lyses);
    }

    /// EN-124 — hostile menaikkan `total_mergers`.
    #[test]
    fn en124_hostile_menaikkan_mergers() {
        let mut c = hostile_setup();
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.hostile_takeover_splice(2, 1, 0);
        assert_eq!(c.macro_state.total_mergers, 1);
        assert_eq!(c.macro_state.total_births, 2); // 2 inkubasi awal
        assert_eq!(c.macro_state.total_lyses, 0);
    }

    /// SEC-110 — `adopt` dengan nonce basi -> `ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED`.
    #[test]
    #[should_panic(expected = "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED")]
    fn sec110_adopt_stale_nonce_ditolak() {
        let mut c = hostile_setup();
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.adopt(1, 2, 99); // nonce target = 0
    }

    /// EN-121 — `get_colony_atp`: Σ ATP koloni aktif.
    #[test]
    fn en121_get_colony_atp() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // atp 50
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // atp 50
        assert_eq!(c.get_colony_atp(ALKEMIS_ID.parse().unwrap()).0, 100);
        // Feed besar menambah ATP strain 1: +4 -> 104. (fee 4e15 ≤ saldo 8.000? tidak —
        // gunakan stake besar agar cukup.)
        let mut c2 = SnaccContract::new();
        let _ = call_incubate(&mut c2, 1_000_000_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        c2.macro_state.params.dormancy_threshold_bps = 0;
        let mut v = c2.strains.get(&1).unwrap().clone();
        v.vitality = constants::VITALITY_CAP;
        let _ = c2.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c2.feed_strain(1, U128(4 * constants::ATP_YOCTO_UNIT));
        assert_eq!(c2.get_colony_atp(ALKEMIS_ID.parse().unwrap()).0, 54);
        // Sel mati tidak dihitung.
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(2);
        assert_eq!(c.get_colony_atp(ALKEMIS_ID.parse().unwrap()).0, 50);
    }
    // ------------------------------------------------------------------
    // Pipeline #6 — get_inventory (J3.8) & get_global_solvency (J3.9)
    // ------------------------------------------------------------------

    /// J3.8 — saldo satu molekul; token/strain tak dikenal = 0 (tanpa panic).
    #[test]
    fn pipeline6_get_inventory_ada_dan_nol() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 3); // 3 spora genesis
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        assert_eq!(
            c.get_inventory(1, "mt:spore-ignis".to_string()).0,
            3
        );
        // Token tak dikenal -> 0, tanpa panic.
        assert_eq!(c.get_inventory(1, "mt:bogus".to_string()).0, 0);
        // Strain tak dikenal -> 0, tanpa panic.
        assert_eq!(c.get_inventory(99, "mt:spore-ignis".to_string()).0, 0);
        // Setelah transmute: spora berkurang, enzim muncul.
        // `grant` MENIMPA (bukan menambah): 3 genesis -> 5 -> burn 2 -> 3.
        grant(&mut c, "mt:spore-ignis", 5);
        // Beri bit Fermenter (extractor + fermenter) agar transmute sah.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert_eq!(c.get_inventory(1, "mt:spore-ignis".to_string()).0, 3);
        assert_eq!(c.get_inventory(1, "mt:enzyme-amalgam".to_string()).0, 2);
    }

    /// J3.9 — `V-04` solvent sepanjang rentetan aksi (inkubasi, fee, feed,
    /// transmute, likuidasi, NPC) + `drift == 0` di setiap titik.
    #[test]
    fn pipeline6_solvency_solvent_setelah_aksi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 2); // supply 10.000
        // Dua bit peran (extract untuk fee + ferment untuk transmute).
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011;
        let _ = c.strains.insert(1, v);
        let s0 = c.get_global_solvency();
        assert!(s0.solvent, "drift {}", s0.drift);
        // Fee katalis (inflow eksternal NEAR-yocto).
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(100));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(100));
        let s1 = c.get_global_solvency();
        assert!(s1.solvent, "drift setelah extract: {}", s1.drift);
        assert_eq!(s1.total_catalyst_fees, 100);
        // Feed (debit saldo strain + rebate hypo).
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.feed_strain(1, U128(1_000));
        assert!(c.get_global_solvency().solvent);
        // Transmute (K2/K3 volume; burn spora, mint enzim — tak sentuh $SNACC).
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert!(c.get_global_solvency().solvent);
        // NPC buy (kas strain -> reserve NPC).
        let _ = c.npc_buy_spores(1, "npc-vulture-ma".to_string(), 1);
        let s4 = c.get_global_solvency();
        assert!(s4.solvent, "drift setelah NPC: {}", s4.drift);
        assert_eq!(s4.sum_npc_reserve, c.get_npc("npc-vulture-ma".to_string()).snacc_reserve);
        // Likuidasi strain lain (bounty -> akun; recovery -> treasury).
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        let s5 = c.get_global_solvency();
        assert!(s5.solvent, "drift setelah likuidasi: {}", s5.drift);
        // Konsistensi dengan `get_macro_state`.
        let macro_view = c.get_macro_state();
        assert_eq!(
            s5.total_minted,
            macro_view.total_snacc_supply + macro_view.total_snacc_burned
        );
        assert_eq!(s5.total_burned, macro_view.total_snacc_burned);
        assert_eq!(s5.sum_treasury, c.treasury);
        assert_eq!(s5.sum_dividend_pool, macro_view.industrial_dividend_pool);
        assert_eq!(s5.epoch_dividend_inflow, macro_view.epoch_dividend_inflow);
    }

    /// J3.9 — pembukuan rusak (disuntik) terdeteksi: `solvent = false`, drift dilaporkan.
    #[test]
    fn pipeline6_solvency_drift_terdeteksi() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert!(c.get_global_solvency().solvent);
        // Simulasi bug pembukuan: agregat dimanipulasi langsung.
        c.macro_state.sum_strain_snacc += 5;
        let s = c.get_global_solvency();
        assert!(!s.solvent);
        assert_eq!(s.drift, -5);
    }
}

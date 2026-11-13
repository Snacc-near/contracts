//! Snacc — core contract of `snacc-v001.near` (M1 `snacc-core` + M2 `snacc-svg`).
//!
//! Four standards in one atomic contract:
//! `NEP-141` ($SNACC) · `NEP-171`/`NEP-177` (Homunculus Strain + live SVG) ·
//! `NEP-245` (9 molecules) · mechanics F1–F7 (M&A, Cell Lysis, TEE zero-trust).
//!
//! Map: [[Snacc - Katalog Fitur]] · [[Snacc - Spec Antarmuka Kontrak]] ·
//! [[Snacc - Model Data]] (D1–D9) · [[Snacc - Backlog Master]].

pub mod events;
pub mod state;
pub mod svg;

use near_sdk::json_types::U128;
use near_sdk::serde::Deserialize;
use near_sdk::store::{LookupMap, LookupSet, Vector};
use near_sdk::{env, near, AccountId, NearToken, PanicOnDefault, Promise, PromiseOrValue};
use state::{
    constants, AlkemisProfile, Epoch, FeederEntry, MacroState, MacroStateView, MacroZone,
    MnaLogEntry, NftMetadataView, NftTokenView, NpcMerchantAgent, NpcQuoteView, OrganelleSlots,
    SolvencyTier, StrainId, StrainRole, StrainStatus, StrainVessel, StrainVesselView, TeeAttestation, TokenId,
};

/// Genome mutation payload — `choices[0].message.content` (JSON inside JSON, `F3`/`I-03`).
/// DK-074 — strict schema: unknown fields are rejected (`deny_unknown_fields`).
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct MutationPayload {
    strain_id: u64,
    efficiency_shift_bps: i64,
    vitality_bonus: u64,
    #[serde(default)]
    phenotype_trait: Option<String>,
}

/// DK-074 — `phenotype_trait` is at most 32 characters (Diagram Konseptual spec).
/// The full whitelist is not yet defined in the vault — length is enforced here;
/// the whitelist will follow once the spec writes it down (recorded decision).
const TEE_TRAIT_MAX_CHARS: usize = 32;

/// Chat response envelope (`/v1/chat/completions`) — anti-replay `chat_id` extraction.
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

// DK-074 NOTE: the envelope/choice (gateway format) is deliberately TOLERANT — gateway
// fields can be added (index, finish_reason, logprobs, …) without breaking verification.
// The STRICT schema (deny_unknown_fields) is enforced on MutationPayload — that decision
// payload is the attack surface.
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

/// Contract state — `D1–D8` (`D9` = event `NEP-297`, no state). `VC-002`.
#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct SnaccContract {
    /// `D1` Alchemist registry.
    pub alchemists: LookupMap<AccountId, AlkemisProfile>,
    /// `D2` Strain ledger & virtual/sub-account balance sheet.
    pub strains: LookupMap<StrainId, StrainVessel>,
    /// `D3` `NEP-245` multi-token inventory per Strain.
    pub inventories: LookupMap<(StrainId, TokenId), u128>,
    /// `D5` Registry of TEE attestation proofs (`ADR-21`).
    pub tee_attestations: LookupMap<String, TeeAttestation>,
    /// `D5` Anti-replay: `chat_id`s already used (`used_tee_chat_ids`).
    pub used_tee_chat_ids: LookupSet<String>,
    /// `D5`/`I-16` (`ADR-33`): registry of TEE enclave `ed25519` public keys (`pubkey -> attested_at_sec`).
    pub attested_tee_signers: LookupMap<[u8; 32], u64>,
    /// Contract owner (gate of `register_attested_signer`) — bootstrapped at first registration.
    pub owner_id: Option<AccountId>,
    /// `F6`: liquidator bounty per **caller account** (`D6` extension) —
    /// now ALSO the per-account `NEP-141` `$SNACC` ledger (`N141-101..106`):
    /// `ft_balance_of(account)` = `account_balances` only (recorded decision).
    pub account_balances: LookupMap<AccountId, u128>,
    /// `N141` — strain index per owner (for `get_total_wealth`; `LookupMap`
    /// cannot be iterated, so the id list is maintained at `incubate_strain`).
    pub strains_by_owner: LookupMap<AccountId, Vector<StrainId>>,
    /// `N141` — MVP storage registry: account -> total deposited milliNEAR.
    pub registered_storage: LookupMap<AccountId, u128>,
    /// `DK-033` — `epoch_dividend_rate[epoch]` (scaled by `DIVIDEND_RATE_SCALE`).
    pub epoch_dividend_rate: LookupMap<Epoch, u128>,
    /// `DK-033` — `(inflow, aether_burned_total)` per closed epoch: exact payout basis
    /// `contribution × inflow / burned` (256-bit mul-div, no scale overflow).
    pub epoch_dividend_totals: LookupMap<Epoch, (u128, u128)>,
    /// `DK-033` — `epoch_aether_burned[(epoch, strain_id)] -> amount`.
    pub epoch_aether_burned: LookupMap<(Epoch, StrainId), u128>,
    /// `DK-033` — double-claim guard for `(epoch, strain_id)`.
    pub dividend_claimed: LookupSet<(Epoch, StrainId)>,
    /// `EN-107` — feed contribution per `(epoch, account)` (Great Feast).
    pub feast_contributions: LookupMap<(Epoch, AccountId), u128>,
    /// `EN-107` — top-3 feeder leaderboard per epoch (capped at 3 entries, gas-safe).
    pub epoch_top_feeders: LookupMap<Epoch, Vec<FeederEntry>>,
    /// `ADR-30` — 3 autonomous on-chain NPC merchants (`D4`).
    pub npc_agents: LookupMap<String, NpcMerchantAgent>,
    /// `D6` `$SNACC` cash per Strain (`vault_subaccount` balance sheet).
    pub snacc_balances: LookupMap<StrainId, u128>,
    /// `D6` `Rule-Bound Treasury` — no admin withdrawal function (`I-05`).
    pub treasury: u128,
    /// `D7` Monetary thermostat parameters & state (`CrucibleTemp`, burst, dividends).
    pub macro_state: MacroState,
    /// `D8` M&A & liquidation log.
    pub mna_logs: Vector<MnaLogEntry>,
    /// `F1` — sequential `StrainId` counter starting at `1`.
    pub next_strain_id: StrainId,
    /// SEC-112/migrate — collection prefix generation (bumped on every `migrate()`;
    /// new collections use `[gen, base]` → old data becomes unreadable).
    pub prefix_gen: u8,
    /// `ADR-22` — is the demo sandbox clock active? Set ONCE in `new()`/`migrate()` from
    /// the contract account (only accounts ending in `.testnet`); no setter → impossible
    /// to activate on mainnet.
    pub sandbox_clock_enabled: bool,
    /// `ADR-22` — `sandbox_time_offset_sec` is added to block time by `now_sec()`.
    pub sandbox_time_offset_sec: u64,
    /// Audit W8 — state-generation tag for the sub-account name `s-<id>-<tag>`:
    /// block height at `new()`/`migrate()` (monotonically increasing) → a `next_strain_id`
    /// restarting after a state reset cannot clash with leftover `s-*` names of old deployments.
    pub subaccount_tag: u64,
}

const GEN_MARKER: &[u8] = b"__snacc_prefix_gen_v1";
/// SEC-113 — owner marker in raw storage (survives migrate, outside collections).
const OWNER_MARKER: &[u8] = b"__snacc_owner_v1";

/// `floor(a × b / d)` with a 256-bit product (no intermediate overflow);
/// result > u128 → saturates to `u128::MAX`. `d == 0` → 0 (callers use `max(d,1)`).
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
    // 256/128 long division (hi < d guarantees the result fits in u128).
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
/// DK-073 — TEE response size cap (spec: "e.g. ≤4 KB").
const TEE_MAX_RESPONSE_BYTES: usize = 4096;

#[near]
impl SnaccContract {
    /// Build the contract state with prefix `[gen, base]` — SEC-112/migrate.
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
        // Audit W6 — epoch 0 ends `EPOCH_DURATION_SEC` after init/migrate.
        c.macro_state.epoch_ends_sec = c.now_sec() + constants::EPOCH_DURATION_SEC;
        c
    }

    /// Audit W8 — Strain sub-account name: `s-<id>-<subaccount_tag>.<contract-account>`
    /// (suffix = contract account: the parent always exists on its own network — Pipeline #7).
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

    /// `ADR-22` — sandbox mode gate: the contract account must be `*.testnet`.
    fn is_sandbox_network() -> bool {
        env::current_account_id().as_str().ends_with(".testnet")
    }

    /// `ADR-22` — the SINGLE source of time (seconds) for BMR/aging/epochs/zones:
    /// block time + `sandbox_time_offset_sec` (0 outside sandbox mode).
    pub(crate) fn now_sec(&self) -> u64 {
        env::block_timestamp() / 1_000_000_000 + self.sandbox_time_offset_sec
    }

    /// Millisecond variant of `now_sec()` (sandbox offset included).
    pub(crate) fn now_ms(&self) -> u64 {
        env::block_timestamp() / 1_000_000 + self.sandbox_time_offset_sec * 1_000
    }

    /// `advance_sandbox_clock` (`ADR-22`, Spec Antarmuka §1): shifts
    /// `sandbox_time_offset_sec += hours * 3600` so judges can test Chapter 11 &
    /// Chapter 7 in one click. Contract owner only, sandbox mode only (`.testnet`).
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

    /// Sandbox mode view: `(sandbox_clock_enabled, sandbox_time_offset_sec, now_sec)`.
    pub fn get_sandbox_clock(&self) -> (bool, u64, u64) {
        (
            self.sandbox_clock_enabled,
            self.sandbox_time_offset_sec,
            self.now_sec(),
        )
    }

    #[init]
    pub fn new() -> Self {
        // gen=0 → prefix [0, base] — already DIFFERENT from the pre-gen contract (bare [base])
        // so old data is unreadable even without the marker. The marker is only for migrate.
        // SEC-113 — the owner is fixed AT deploy time (the deployer), not the first caller
        // of any method (that pattern would open an owner takeover).
        let mut c = Self::build(0);
        c.owner_id = Some(env::predecessor_account_id());
        env::storage_write(OWNER_MARKER, c.owner_id.as_ref().unwrap().as_bytes());
        c.spawn_npcs();
        c
    }

    /// `ADR-30` — seed 3 autonomous NPCs (`D4` Model Data): genesis stock = target stock
    /// (recorded decision), reserve 0, base price `NPC_P_BASE`.
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

    /// Protocol version marker — used by smoke tests & deploy verification.
    pub fn protocol_version(&self) -> &'static str {
        "snacc-v001"
    }

    /// `F1` / `P1` — mint a Genesis `Homunculus Strain` (`NEP-171`).
    ///
    /// `initial_snacc_stake` is the `80/20` split amount. The stake source must be real:
    /// attached NEAR (amount `0` or == deposit; supply is born from the NEAR stake) or,
    /// when the deposit is `0`, a debit of the caller's `$SNACC` balance (no new mint).
    #[payable]
    pub fn incubate_strain(
        &mut self,
        role: StrainRole,
        initial_snacc_stake: U128,
        initial_spores: u64,
    ) -> StrainVesselView {
        // `I-08`: `StrainRole` guarantees a single role bit; the assert re-checks the bitmask.
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

        // Audit W5/W7 — the stake source MUST be real (closes free minting, `I-01`):
        // (a) attached NEAR > 0 → supply is born from the NEAR stake (recorded decision:
        //     faucet & `ft_transfer_call` superseded; onboarding path for new players);
        //     argument 0 or == deposit.
        // (b) attached 0 → the stake is debited from the caller's `$SNACC` balance
        //     (`account_balances`: lysis bounty / M&A payout) — supply does NOT grow.
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
        // `80%` → Strain working capital (rounding remainder keeps `I-01` conservation).
        let to_treasury = stake * constants::STAKE_TREASURY_BPS / constants::BPS_DENOMINATOR;
        let to_strain = stake - to_treasury;

        let owner_id = caller;
        let now_sec = self.now_sec();
        let strain_id = self.next_strain_id;
        self.next_strain_id += 1;

        // Pipeline #7 — the suffix is derived from the contract account (parent ALWAYS exists):
        // hardcoding ".snacc-v001.near" fails on testnet (parent not registered ->
        // `create_account` would fail on-chain).
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

        // `D2` ledger, `D6` internal balance sheet & treasury, `D7` `$SNACC` supply (`I-01`).
        let _ = self.strains.insert(strain_id, vessel.clone());
        self.set_strain_snacc(strain_id, to_strain);
        self.treasury += to_treasury;
        if mint_new_supply {
            self.macro_state.total_snacc_supply += stake;
            // `N141-105` — NEP-297 event with standard `nep141` (`ft_mint`):
            // the only true `$SNACC` mint path (`new` mints nothing; every other
            // credit — rebate, dividend claim, lysis bounty, M&A/adopt payment,
            // NPC trade, `ft_transfer` — moves existing balances between
            // ledgers and leaves `total_snacc_supply` unchanged). Amount is a
            // string per the NEP-141 standard.
            events::emit_standard_event(
                "nep141",
                "ft_mint",
                &format!(
                    r#"{{"owner_id":"{}","amount":"{}","memo":"incubate_strain"}}"#,
                    owner_id, stake
                ),
            );
        }
        if self.macro_state.current_epoch == 0 {
            self.macro_state.epoch_treasury_initial += to_treasury;
        }

        // `D3` — `mt_mint` of `NEP-245` genesis spores into `(strain_id, token_id)` inventory.
        let spore_key: (StrainId, TokenId) =
            (strain_id, constants::GENESIS_SPORE_TOKEN_ID.to_string());
        let spores = self.inventories.get(&spore_key).copied().unwrap_or(0);
        let _ = self
            .inventories
            .insert(spore_key, spores + initial_spores as u128);

        // `EN-124` — demographics counter: incubation = birth.
        self.macro_state.total_births = self.macro_state.total_births.saturating_add(1);
        // `D9` — emit NEP-297 `strain_created`.
        events::emit_event_json(&format!(
            r#"{{"kind":"strain_created","strain_id":{},"owner_id":"{}","role_mask":{},"initial_snacc_stake":{},"spores_minted":{}}}"#,
            strain_id, owner_id, role_mask, stake, initial_spores
        ));

        // `D1` — alchemist registry (`joined_sec` on first, `strain_count += 1`).
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

        // `N141` — strain index per owner (for `get_total_wealth`);
        // the `Vector` prefix must be unique per account (near-sdk nested-collection pattern).
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

    /// `F1` / `P1` (`ADR-02`, `ADR-24`) — create a real NEAR sub-account.
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

        // SEC-111 (triage Option a) — the `pending_callbacks` lock becomes REAL: set to 1
        // during the promise window; `assert_atomic_ready` rejects M&A mutations mid-window;
        // reset to 0 in the `on_subaccount_created` callback (all paths).
        let mut locked = vessel.clone();
        locked.pending_callbacks = 1;
        locked.record_mutation();
        let _ = self.strains.insert(strain_id, locked);

        // Audit W8 — name from the ledger (unique across state generations; see `subaccount_for`).
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

    /// `ADR-24` — anti-spoofing callback (`#[private]`). A failed
    /// `create_account` does not corrupt the internal balance sheet; the NEAR deposit is refunded.
    #[private]
    pub fn on_subaccount_created(
        &mut self,
        strain_id: StrainId,
        sub_account_id: AccountId,
        owner_id: AccountId,
        deposit_yocto: U128,
    ) {
        // SEC-111 — release the `pending_callbacks` lock on ALL paths (window closed).
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

    /// `F1` — read-only view of one Strain with `effective_vitality` at `now`.
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

    /// `F4` / `ADR-04` — macro `CrucibleTemp` temperature (`mK`) with cron-free lazy decay:
    /// `floor(1000 * V_decayed / max(total_snacc_supply, 1))`.
    pub fn get_crucible_temp(&self) -> u64 {
        let now_ms = self.now_ms();
        self.macro_state.crucible_temp_at(now_ms)
    }

    /// `F4` — `MacroState` view with `crucible_temp` computed at the current timestamp.
    pub fn get_macro_state(&self) -> MacroStateView {
        let now_ms = self.now_ms();
        self.macro_state.to_view(now_ms)
    }

    /// `F4` — epoch transition (`VC-005` + `DK-033` + `EN-01`):
    /// 1. Close the dividend epoch: `epoch_dividend_rate[epoch] = epoch_dividend_inflow × SCALE
    ///    / max(epoch_aether_burned_total, 1)` is computed at close time (order-of-tx
    ///    manipulation-proof), `industrial_dividend_pool += epoch_dividend_inflow`, counters reset,
    ///    `last_closed_epoch += 1`, emit `epoch_dividend_closed`.
    /// 2. Reset the `I-04` treasury quota (`epoch_treasury_spent = 0`, snapshot `initial = treasury`).
    /// 3. Reset the `EN-104` burst discount accumulator; bursts expire automatically via
    ///    `active_burst_bps` (keeperless — `ADR-27`).
    pub fn advance_epoch(&mut self) {
        // Audit W6 — time gate: an epoch can only be closed after it has ended
        // (anti-spam: resets the I-04 quota, closes dividends, expires claims).
        let now_sec = self.now_sec();
        assert!(
            now_sec >= self.macro_state.epoch_ends_sec,
            "{}",
            constants::ERR_EPOCH_NOT_ENDED
        );
        // The next epoch is computed from `now` (a late keeper cannot close
        // several epochs in a row within one block).
        self.macro_state.epoch_ends_sec = now_sec + constants::EPOCH_DURATION_SEC;
        // --- `DK-033`: dividend epoch close (V-02: time from the block, not a parameter). ---
        let closing = self.macro_state.current_epoch;
        let total_burned = self.macro_state.epoch_aether_burned_total;
        // Live bug (failed `advance_epoch` tx): `inflow × 1e18` overflows u128 for
        // inflow ≥ ~3.4e20 → 256-bit mul-div. `rate` is informational only (saturates at
        // u128::MAX); claim payouts use `epoch_dividend_totals` (exact).
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

        // --- `EN-105`: evaluate The Great Feast (Goldilocks band) when the epoch closes. ---
        // Recorded decision: target = 5% of supply, band ±20% (FEAST_TARGET_BPS/FEAST_BAND_BPS);
        // the MVP feast only grades & emits the verdict — token bonuses are not nominalized.
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

        // --- `VC-005`: reset the `I-04` quota. ---
        self.macro_state.current_epoch = self.macro_state.current_epoch.saturating_add(1);
        self.macro_state.epoch_treasury_spent = 0;
        self.macro_state.epoch_treasury_initial = self.treasury;

        // --- `EN-104`: reset the burst discount accumulator. ---
        self.macro_state.epoch_burst_discount_granted = 0;

        events::emit_event_json(&format!(
            r#"{{"kind":"epoch_dividend_closed","epoch":{},"pool":{},"total_burned":{},"rate":{}}}"#,
            closing,
            self.macro_state.industrial_dividend_pool,
            total_burned,
            rate
        ));
    }

    /// `F4` / `I-01` — burn `$SNACC` from the `snacc_balances[strain_id]` balance sheet.
    /// TODO(VC-006/NEP-141): align the `ft_burn(amount: U128)` signature once the global NEP-141 ledger exists.
    pub fn ft_burn(&mut self, strain_id: StrainId, amount: U128) {
        // `SEC-101` (`free-pi-cli` finding 2026-11-07): the caller MUST be checked — only
        // the Strain owner (*token holder*) may burn its own balance.
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
        // `N141-105` — NEP-297 event with standard `nep141` (`ft_burn`):
        // `amount` is a decimal string (u128) and `memo` is `null` per the
        // NEP-141 events standard (a JSON number loses precision above 2^53).
        events::emit_standard_event(
            "nep141",
            "ft_burn",
            &format!(
                r#"{{"owner_id":"{}","amount":"{}","memo":null}}"#,
                self.strains
                    .get(&strain_id)
                    .map(|v| v.owner_id.to_string())
                    .unwrap_or_default(),
                amount.0
            ),
        );
    }

    // =====================================================================
    // VC-004 — `F2`: 3-stage `NEP-245` supply chain + Lazy BMR (`ADR-12/13`)
    // =====================================================================

    /// Pipeline #6 — the ONLY write path of `snacc_balances`: maintains the
    /// `sum_strain_snacc` aggregate for invariant `V-04` (LookupMap cannot be iterated).
    fn set_strain_snacc(&mut self, strain_id: StrainId, new_bal: u128) {
        let old = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        let delta = new_bal as i128 - old as i128;
        self.macro_state.sum_strain_snacc =
            (self.macro_state.sum_strain_snacc as i128 + delta).max(0) as u128;
        let _ = self.snacc_balances.insert(strain_id, new_bal);
    }

    /// Pipeline #6 — write path of `account_balances` + the `sum_account_balances` aggregate.
    fn set_account_balance(&mut self, account: &AccountId, new_bal: u128) {
        let old = self.account_balances.get(account).copied().unwrap_or(0);
        let delta = new_bal as i128 - old as i128;
        self.macro_state.sum_account_balances =
            (self.macro_state.sum_account_balances as i128 + delta).max(0) as u128;
        let _ = self.account_balances.insert(account.clone(), new_bal);
    }

    /// `EN-114`/§13a — deduct the strain's ATP; `ERR_NOT_ENOUGH_ATP` when short.
    /// ATP is non-transferable: used only as an internal work cost.
    fn pay_atp(v: &mut StrainVessel, cost: u128) {
        assert!(
            v.atp_balance >= cost,
            "{}",
            constants::ERR_NOT_ENOUGH_ATP
        );
        v.atp_balance -= cost;
    }

    /// `EN-115`/§13a — `atp_minted = (fee / ATP_YOCTO_UNIT) × efficiency_bps/10000
    /// × (10000 + 1000 × level_mitokondria)/10000` (mitochondria +10%/level).
    /// Note: the `zone` component of EN-06 is not included (§13a final, zoneless).
    fn calc_atp_minted(fee_yocto: u128, efficiency_bps: u16, level_mitokondria: u8) -> u128 {
        let base = fee_yocto / constants::ATP_YOCTO_UNIT;
        base * u128::from(efficiency_bps) / constants::BPS_DENOMINATOR
            * u128::from(10_000 + 1_000 * u16::from(level_mitokondria))
            / constants::BPS_DENOMINATOR
    }

    /// `ECON-111` FINAL — add the K2/K3 legs of `V_epoch` (econ-report-3 §1):
    /// `unit × VOLUME_SCALE(5) × P_BASE(100)` for mt_burn & mt_mint.
    /// K4 (liquidation cash) is added directly in `harvest_starving_strain`.
    fn add_volume_legs(&mut self, burn_units: u128, mint_units: u128) {
        let legs = (burn_units + mint_units)
            * constants::VOLUME_SCALE
            * constants::P_BASE;
        self.macro_state.volume_epoch = self.macro_state.volume_epoch.saturating_add(legs);
    }

    /// `Lazy BMR` gate (`F2`): reject when `effective_vitality == 0` (`ERR_STRAIN_STARVING`).
    fn assert_not_starving(v: &StrainVessel, now_sec: u64) -> u64 {
        let eff = v.effective_vitality(now_sec);
        assert!(eff > 0, "{}", constants::ERR_STRAIN_STARVING);
        eff
    }

    /// `V-01` (audit W4) — only the Strain owner may use its ATP/inventory/
    /// metabolism (`ERR_NOT_STRAIN_OWNER`, Spec Dividend §5).
    fn assert_strain_owner(v: &StrainVessel) {
        assert_eq!(
            env::predecessor_account_id(),
            v.owner_id,
            "{}",
            constants::ERR_NOT_STRAIN_OWNER
        );
    }

    /// Caste gate: `role_mask` must contain the role bit (`ERR_ROLE_NOT_ALLOWED`).
    fn assert_role(v: &StrainVessel, role_bit: u8) {
        assert!(
            v.role_mask & role_bit != 0,
            "{}",
            constants::ERR_ROLE_NOT_ALLOWED
        );
    }

    /// `$SNACC` catalyst (`F2`/`F4`, `VC-005`):
    /// - Validate `attached_deposit == fee` (`ERR_STAKE_DEPOSIT_MISMATCH`).
    /// - Update `volume_epoch` (including lazy decay since `last_update_ms`) + `last_update_ms`.
    /// - Compute `temp = get_crucible_temp()`:
    ///   * `temp > 1400` (Calcination Fever): `burn = fee * 350 / 10000`, `total_snacc_supply -= burn`,
    ///     `total_snacc_burned += burn` (`I-01`), the remaining `fee - burn` goes to `treasury`.
    ///   * `temp < 600` (Hypothermia): `fee` goes to `treasury`; `rebate = fee * 200 / 10000`
    ///     capped by the remaining epoch quota `epoch_treasury_initial * 500 / 10000 - epoch_treasury_spent` (`I-04`),
    ///     paid from `treasury` to `snacc_balances[strain_id]`.
    ///   * `600..=1400` (middle zone): `fee` goes to `treasury` with no burn/rebate.
    fn take_catalyst_fee(&mut self, strain_id: StrainId, fee: &U128) {
        assert_eq!(
            env::attached_deposit().as_yoctonear(),
            fee.0,
            "{}",
            constants::ERR_STAKE_DEPOSIT_MISMATCH
        );
        // Pipeline #6 — an EXTERNAL fee (attached NEAR-yocto) = real inflow for V-04;
        // the INTERNAL feed_strain fee is not counted (see the SEC-112/VI note above).
        self.macro_state.total_catalyst_fees = self
            .macro_state
            .total_catalyst_fees
            .saturating_add(fee.0);
        self.apply_thermostat_fee(strain_id, fee.0, 0);
    }

    /// Thermostat path for internal fees (`feed_strain` `EN-103`): same as
    /// `take_catalyst_fee` without the attached-deposit assertion.
    fn apply_thermostat_fee(&mut self, strain_id: StrainId, fee: u128, extra_volume: u128) {
        // SEC-112/VI NOTE: `total_catalyst_fees` counts only EXTERNAL fees
        // (attached yocto — via `take_catalyst_fee`); the INTERNAL `feed_strain` fee
        // (from the strain's balance) is not counted as inflow (already internally conserved).
        let now_ms = self.now_ms();
        let v_decayed = self.macro_state.decayed_volume(now_ms);
        // EN-117 (owner decision: temperature is literal ATP) — `extra_volume` =
        // `atp_minted × ATP_VOLUME_SCALE` from the feed; it lands in the same epoch's V_epoch.
        self.macro_state.volume_epoch = v_decayed
            .saturating_add(fee)
            .saturating_add(extra_volume);
        self.macro_state.last_update_ms = now_ms;

        let temp = self.get_crucible_temp();
        // `EN-106` — macro zone transition detection → `zone_changed` event (keeperless).
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
            // `LC-11` — `mt:elixir-homeostasis` shield (molecule catalog): holders of
            // >=1 unit get a 50% discount on the Fever burn — PER STRAIN (recorded
            // decision; the elixir is not consumed, passive effect).
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
            // `DK-033` (`ADR-28`): 50% of the non-burned fee goes to the dividend pool, the rest to the treasury.
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
    /// `inflow = (fee - burned) × DIVIDEND_INFLOW_BPS / 10000` goes into `epoch_dividend_inflow`;
    /// the rest (`net - inflow`) goes to the `Rule-Bound Treasury`. The dividend pool is separate from
    /// the `I-04` rebate quota (Spec Dividend §3).
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

    /// Metabolic recovery (`F2`): `vitality = min(10000, effective + 500)`, `last_fed_sec = now`.
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

    /// `mt_burn` — decrease inventory (`ERR_INSUFFICIENT_BALANCE` when short).
    fn inv_burn(&mut self, strain_id: StrainId, token: &str, amount: u128) {
        let key = (strain_id, token.to_string());
        let bal = self.inventories.get(&key).copied().unwrap_or(0);
        assert!(bal >= amount, "{}", constants::ERR_INSUFFICIENT_BALANCE);
        let _ = self.inventories.insert(key, bal - amount);
    }

    /// `mt_mint` — add to inventory.
    fn inv_mint(&mut self, strain_id: StrainId, token: &str, amount: u128) {
        let key = (strain_id, token.to_string());
        let bal = self.inventories.get(&key).copied().unwrap_or(0);
        let _ = self.inventories.insert(key, bal + amount);
    }

    /// `F2` Stage 1 — `extract_spores` (requires `ROLE_EXTRACTOR`).
    /// Documented ambiguity: the extraction output amount is not nominalized in the spec
    /// → **1 unit per call** (VC-004 review decision, 2026-11-07).
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
        // `EN-116`/§13a — ATP cost of extract = 1.
        Self::pay_atp(&mut v, 1);
        self.take_catalyst_fee(strain_id, &snacc_catalyst_fee);
        self.inv_mint(strain_id, &spore_type, 1);
        // `ECON-111` — K3 leg: mt_mint of 1 spore unit.
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

    /// `F2` Stage 2 — `transmute_spores_to_enzyme` (`mt_burn` spores → `mt_mint` enzymes
    /// `floor(spores_to_burn * efficiency_bps / 10000)`, requires `ROLE_FERMENTER`).
    /// Documented ambiguity: the signature has no spore/enzyme type → mapped to
    /// **per-batch recipe ratios (2 spores)** with priority: amalgam → redactase → synthase.
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
        let k = burn / 2; // recipe batch = 2 spores (Molecule Catalog)
        let ignis = constants::SPORE_TOKEN_IDS[0];
        let cryo = constants::SPORE_TOKEN_IDS[1];
        let lumin = constants::SPORE_TOKEN_IDS[2];
        // Recipe priority: `amalgam` (2x ignis) → `redactase` (1x ignis + 1x lumin) → `synthase` (2x cryo).
        let recipe: (&str, [(&str, u128); 2]) = if self.inv_balance(strain_id, ignis) >= burn {
            (constants::ENZYME_AMALGAM, [(ignis, burn), ("", 0)])
        } else if self.inv_balance(strain_id, ignis) >= k && self.inv_balance(strain_id, lumin) >= k {
            (constants::ENZYME_REDACTASE, [(ignis, k), (lumin, k)])
        } else if self.inv_balance(strain_id, cryo) >= burn {
            (constants::ENZYME_SYNTHASE, [(cryo, burn), ("", 0)])
        } else {
            env::panic_str(constants::ERR_INSUFFICIENT_BALANCE)
        };
        // `EN-110` — per-caste per-zone diet matrix (connection map: EN-04 → transmute).
        // Recorded decision: Fever → Extractor −20%; Famine (Hypothermia) → Synthesizer
        // −20%; otherwise 0. Multi-role (splice result): one matching bit is enough.
        // `snacc_catalyst_fee` = the nominal amount; the attached deposit must equal the fee AFTER the discount.
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
        // `EN-116`/§13a — ATP cost of transmute = 2.
        Self::pay_atp(&mut v, 2);
        self.take_catalyst_fee(strain_id, &U128(effective_fee));
        for (token, amount) in recipe.1 {
            if amount > 0 {
                self.inv_burn(strain_id, token, amount);
            }
        }
        // EN-119 — Nucleus: production output +10%/level (owner decision 2026-11-08).
        let nuk_bps = u128::from(v.organel.nukleus)
            * constants::NUCLEUS_OUTPUT_BPS_PER_LEVEL;
        let minted = burn
            * u128::from(v.efficiency_bps)
            * (constants::BPS_DENOMINATOR + nuk_bps)
            / (constants::BPS_DENOMINATOR * constants::BPS_DENOMINATOR);
        self.inv_mint(strain_id, recipe.0, minted);
        // `ECON-111` — K2 leg (spores burned) & K3 (enzymes minted).
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

    /// `F2` Stage 3 — `synthesize_elixir` (requires `ROLE_SYNTHESIZER`).
    /// `mt:elixir-aether` can now be synthesized if the strain **has had a successful TEE
    /// mutation** (`phenotype_trait.is_some()` — pipeline decision `DK-044`, 2026-11-07);
    /// TODO(F3): tighten the gate to per-batch TEE attestation (not yet nominalized in the spec).
    /// Documented ambiguity: the elixir output amount is not nominalized →
    /// **1 elixir per recipe batch** (`enzymes_to_burn` must be a multiple of the batch).
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
        // Per-batch recipe (Molecule Catalog): (enzyme_a, per_a, enzyme_b, per_b, batch_total).
        // `mt:elixir-aether`: TEE gate (past successful mutation), recipe = recorded decision
        // (follows the homunculus recipe shape: 2 amalgam + 1 synthase per batch).
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
        // `EN-116`/§13a — ATP cost of synthesize = 3.
        Self::pay_atp(&mut v, 3);
        self.take_catalyst_fee(strain_id, &snacc_catalyst_fee);
        self.inv_burn(strain_id, enz_a, need_a);
        self.inv_burn(strain_id, enz_b, need_b);
        // EN-119 — Nucleus: elixir output +10%/level (owner decision).
        let nuk_bps = u128::from(v.organel.nukleus)
            * constants::NUCLEUS_OUTPUT_BPS_PER_LEVEL;
        let minted_elixir = batches
            * (constants::BPS_DENOMINATOR + nuk_bps)
            / constants::BPS_DENOMINATOR;
        self.inv_mint(strain_id, &elixir_type, minted_elixir);
        // `ECON-111` — K2 leg (enzymes burned) & K3 (elixirs minted).
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
    /// Decode hex into a fixed-size array (`N` bytes) — no extra dependencies.
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

    /// Verify the enclave signature over `<model>:<req_sha256>:<res_sha256>` (`S2.6`).
    /// Pure function so the **real** signature vector can be tested directly.
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

    /// Parse `choices[0].message.content` + `I-03` guardrail (split out for testability).
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

    /// Anti-replay key: `chat_id`/`id` from the body; fallback `res_sha256` (unique per response).
    /// Documented ambiguity: the spec does not name the response id field.
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

    /// `F3` / `P3` (`ADR-21`) — **Phase 1**: lock the request's `SHA-256` commitment
    /// (`commit_genome_experiment`) before calling the TEE enclave.
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
        let _ = Self::hex_to_bytes::<32>(&req_sha256_hex); // validate the 32-byte hex format
        v.pending_commitment = Some(req_sha256_hex.clone());
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"genome_committed","strain_id":{},"req_sha256_hex":"{}"}}"#,
            strain_id, req_sha256_hex
        ));
    }

    /// `F3` / `P3` (`ADR-21`, `I-03`, `I-16`) — **Phase 2**: `apply_tee_mutation_zero_trust`.
    ///
    /// Documented ambiguity (`VC-006` decision): the `tee_pubkey_hex` parameter
    /// was **added** outside the spec table — verification against the `attested_tee_signers`
    /// registry (`I-16`) is impossible without the verifier's key.
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
        // Phase 1 binding: `pending_commitment` must match (`ADR-21`).
        assert_eq!(
            v.pending_commitment.as_deref(),
            Some(req_sha256_hex.as_str()),
            "{}",
            constants::ERR_COMMITMENT_MISMATCH
        );
        v.pending_commitment = None;
        // DK-073 — the response size cap is checked FIRST (before hash/sig)
        // so a giant body cannot burn gas on hashing/parsing.
        assert!(
            raw_response_body.len() <= TEE_MAX_RESPONSE_BYTES,
            "{}",
            constants::ERR_TEE_RESPONSE_TOO_LARGE
        );
        // Recompute `res_sha256` on-chain (`ADR-21` — zero-trust).
        let res_sha256_hex = Self::hex_encode(&env::sha256(raw_response_body.as_bytes()));
        // `I-16` (`ADR-33`): the key must be registered in `attested_tee_signers`.
        let pk = Self::hex_to_bytes::<32>(&tee_pubkey_hex);
        assert!(
            self.attested_tee_signers.get(&pk).is_some(),
            "{}",
            constants::ERR_SIGNER_NOT_REGISTERED
        );
        // Verify the enclave signature over `<model>:<req_sha256>:<res_sha256>`.
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
        // Parse the payload + `I-03` guardrail, then apply the mutation to the Strain.
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

    /// `ADR-33` / `I-16` — TEE enclave public key registry (`register_attested_signer`).
    /// SEC-113 — contract owner only (the owner is fixed at deploy/migrate);
    /// the first-caller-becomes-owner pattern was REMOVED. Overwriting a registered key with
    /// a different attestation is rejected (not silently, `I-16`).
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
        // DK-070 — `signer_added` event (exact name from the Diagram Konseptual spec).
        events::emit_event_json(&format!(
            r#"{{"kind":"signer_added","pubkey_hex":"{}","attested_at_sec":{}}}"#,
            pubkey_hex, attested_at_sec
        ));
    }

    /// DK-070 — `remove_attested_signer`: contract owner only; the key must
    /// be registered; `signer_removed` event (exact name from the spec).
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
    /// Merger tax of `2.5%` of the `absorbed` balance goes to the `Rule-Bound Treasury`; the remaining `97.5%`
    /// `$SNACC` + `100%` of the nine `NEP-245` molecules + `role_mask` are absorbed by `dominant`
    /// (`dominant.role_mask |= absorbed.role_mask`, `I-08`), `generation + 1`,
    /// `efficiency_bps + 800`; `absorbed` -> `SplicedBurned`.
    /// TODO(EN-120): organel set inheritance + mandatory ATP reallocation; `MAX_GENERATION` is enforced (audit W10).
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
        // Audit W10 — the status of BOTH cells is checked (previously only `absorbed`): a dominant
        // already Liquidated/SplicedBurned must not absorb another cell.
        for s in [&dom.status, &abs.status] {
            assert!(
                !matches!(s, StrainStatus::SplicedBurned | StrainStatus::Liquidated),
                "{}",
                constants::ERR_STRAIN_ALREADY_SPLICED
            );
        }
        // Audit W10 — a Tier 3 dominant (effective vitality 0) cannot melt down
        // (the permissionless lysis path must not be dodged via fusion).
        let now_sec = self.now_sec();
        let dom_eff = Self::assert_not_starving(&dom, now_sec);
        // `EN-124` — `MAX_GENERATION` is enforced on every path that creates a new generation.
        let new_gen = dom.generation.saturating_add(1);
        assert!(
            new_gen <= constants::MAX_GENERATION,
            "{}",
            constants::ERR_MAX_GENERATION
        );
        // Merger tax of `2.5%` (`250 bps`) of the `absorbed` balance (`ADR-25`).
        let abs_bal = self.snacc_balances.get(&absorbed_id).copied().unwrap_or(0);
        let tax = abs_bal * constants::MERGER_TAX_BPS as u128 / constants::BPS_DENOMINATOR;
        self.treasury = self.treasury.saturating_add(tax);
        let dom_bal = self.snacc_balances.get(&dominant_id).copied().unwrap_or(0);
        self.set_strain_snacc(dominant_id, dom_bal.saturating_add(abs_bal - tax));
        self.set_strain_snacc(absorbed_id, 0);
        // `100%` of the nine `NEP-245` molecules is absorbed into `dominant`.
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
        // Bitmask `I-08`, `Gen + 1`, `+800 bps` efficiency.
        dom.role_mask |= abs.role_mask;
        dom.generation = new_gen;
        dom.efficiency_bps = dom.efficiency_bps.saturating_add(800);
        // LC-06 — fusion: `BMR −15%` (`SPLICE_BMR_REDUCTION_BPS`). Effective vitality
        // is settled to `now` first so the new BMR does not apply retroactively (Lazy BMR).
        dom.vitality = dom_eff;
        dom.last_fed_sec = now_sec;
        dom.bmr_per_hour = dom.bmr_per_hour
            * (10_000 - constants::SPLICE_BMR_REDUCTION_BPS)
            / 10_000;
        // `EN-116`/§13a — ATP cost of merger = 5 (charged to the dominant).
        Self::pay_atp(&mut dom, 5);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"splice","cost":5}}"#, dominant_id
        ));
        // `EN-120` — endosymbiosis: each organel slot = max(dominant, absorbed);
        // ATP reallocation reopens (`last_allocate_epoch` is reset).
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
        // `EN-124` — demographics counter: splice = merger.
        self.macro_state.total_mergers = self.macro_state.total_mergers.saturating_add(1);
        events::emit_event_json(&format!(
            r#"{{"kind":"strains_spliced","dominant_id":{},"absorbed_id":{},"merger_tax":{}}}"#,
            dominant_id, absorbed_id, tax
        ));
        self.get_strain(dominant_id)
    }
}


// =====================================================================
// TASK 2 #1 — `F5` Path 2: `hostile_takeover_splice` — Chapter 11 Hostile M&A
// (`ADR-26`, `I-09`, `I-14`, `ADR-29`)
// =====================================================================
#[near]
impl SnaccContract {
    /// `F5` Path 2 / `P5` (`ADR-26`, `I-09`, `I-14`) — `hostile_takeover_splice`.
    ///
    /// Cross-account: the caller must own the **raider** (`V-01`); the target can belong to anyone.
    /// Valid when `1 <= effective_vitality(target) <= 2000` (`Tier2Chapter11Distressed`,
    /// `ERR_NOT_DISTRESSED`) and the target is not already Liquidated/SplicedBurned.
    /// `I-14` (`ADR-29`): `target.state_nonce == expected_target_nonce &&
    /// pending_callbacks == 0` (the `expected_target_nonce` parameter was added per
    /// `I-14`; `now_sec` from spec §5.1 is NOT taken as a parameter — per `V-02` the time
    /// must come from the block). Anti-spoofing uses the existing `assert_atomic_ready`.
    ///
    /// Payment: `book_value = target.snacc_balance + Σ(inventory × MOLECULE_FLOOR_PRICE)`;
    /// the raider pays `book_value + 15%` (`HOSTILE_PREMIUM_BPS`); `95%` goes to the old owner
    /// (`account_balances`), `5%` to the `Rule-Bound Treasury`. The raider absorbs 100% of the balance +
    /// inventory + `role_mask |=`; `target.status = SplicedBurned`; efficiency = max of
    /// the two (recorded decision — the spec only says "merge efficiency").
    /// DESIGN NOTE: the direct margin can be negative when the assets' market value < 115% of salvage —
    /// that is exactly the restructuring premium; what is bought is the `role_mask`, the generation, and
    /// the asset-turnover option, not a balance arbitrage.
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
        // `I-09` / `ADR-26` — Tier 2 Chapter 11 Distressed window.
        let now_sec = self.now_sec();
        let eff = target.effective_vitality(now_sec);
        assert!(
            (1..=2000).contains(&eff),
            "{}",
            constants::ERR_NOT_DISTRESSED
        );
        // `I-14` (`ADR-29`) — anti ERC-6551 seller-drain (nonce + callback lock).
        target.assert_atomic_ready(expected_target_nonce);
        // Audit W10 / `EN-124` — `MAX_GENERATION` on the hostile path too; a raider
        // already Liquidated/SplicedBurned cannot absorb.
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

        // Book value (`floor_price` = 1 per molecule unit — recorded decision).
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

        // Payment split: 95% to the old owner, 5% to the treasury (`ADR-26`).
        let seller_payout = payment * constants::HOSTILE_SELLER_BPS / constants::BPS_DENOMINATOR;
        let treasury_cut = payment - seller_payout;
        self.set_strain_snacc(raider_strain_id, raider_bal - payment + target_bal);
        self.set_strain_snacc(distressed_target_id, 0);
        let seller = self
            .account_balances
            .get(&target.owner_id)
            .copied()
            .unwrap_or(0);
        // Credit through the tracked setter so `sum_account_balances` stays consistent
        // with the cash invariant (parity with the deployed contract).
        self.set_account_balance(&target.owner_id, seller.saturating_add(seller_payout));
        self.treasury = self.treasury.saturating_add(treasury_cut);

        // Absorb 100% of the `NEP-245` inventory + `role_mask |=` (`I-08`) + Gen+1 + max efficiency.
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
        // `EN-116`/§13a — ATP cost of takeover = 10 (charged to the raider).
        Self::pay_atp(&mut raider, 10);
        events::emit_event_json(&format!(
            r#"{{"kind":"atp_spent","strain_id":{},"purpose":"hostile","cost":10}}"#, raider_strain_id
        ));
        // `EN-120` — hostile endosymbiosis: organel = max per slot; ATP reallocation
        // reopens (`last_allocate_epoch` is reset).
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

        // `D8` — M&A log.
        self.mna_logs.push(MnaLogEntry {
            timestamp_ms: self.now_ms(),
            kind: state::MnaKind::HostileTakeover,
            actor_id: caller,
            target_id: distressed_target_id,
            amount: payment,
        });
        // `EN-124` — demographics counter: hostile takeover = merger.
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
    /// Permissionless; valid only when `effective_vitality(target) == 0` (`Tier3Chapter7Insolvent`).
    /// Cash split `90/10`: `10%` `$SNACC` bounty to the caller account, `90%` to the
    /// `Rule-Bound Treasury`. All molecule inventory is burned; `status -> Liquidated`.
    /// TODO(EN-101/EN-102): the `90%` nutrient-burst accumulator share; TODO(VC-009): red SVG.
    /// Documented ambiguity: the bounty is credited to the caller's `account_balances`
    /// (spec F6: "credited to the caller's account"; the global NEP-141 ledger does not exist yet).
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
        // `I-10`: only `Tier3Chapter7Insolvent` (`effective_vitality == 0`).
        assert_eq!(
            v.effective_vitality(now_sec),
            0,
            "{}",
            constants::ERR_STRAIN_NOT_INSOLVENT
        );
        let bal = self.snacc_balances.get(&target_id).copied().unwrap_or(0);
        let bounty = bal * constants::LIQUIDATOR_BOUNTY_BPS as u128 / constants::BPS_DENOMINATOR;
        let recovery = bal - bounty;
        // `10%` bounty to the caller account; `90%` to the `Rule-Bound Treasury`.
        let caller_bal = self.account_balances.get(&caller).copied().unwrap_or(0);
        self.set_account_balance(&caller, caller_bal + bounty);
        self.treasury = self.treasury.saturating_add(recovery);
        self.set_strain_snacc(target_id, 0);
        // Molecule inventory is burned entirely (DK-054).
        for token in constants::ALL_TOKEN_IDS {
            let _ = self.inventories.insert((target_id, token.to_string()), 0);
        }
        v.status = StrainStatus::Liquidated;
        v.record_mutation();
        let _ = self.strains.insert(target_id, v);
        // `EN-124` — demographics counter: harvest = lysis.
        self.macro_state.total_lyses = self.macro_state.total_lyses.saturating_add(1);
        // `ECON-111` — K4 leg: cash that changes hands in a liquidation (bounty + recovery).
        self.macro_state.volume_epoch = self
            .macro_state
            .volume_epoch
            .saturating_add(bounty + recovery);
        events::emit_event_json(&format!(
            r#"{{"kind":"strain_liquidated","target_id":{},"liquidator":"{}","bounty":{},"treasury_recovery":{}}}"#,
            target_id, caller, bounty, recovery
        ));
        // `EN-102` — `Nutrient Burst`: 90% of the liquidation proceeds powers a global
        // feed discount for 1 epoch. `scaled_share` = the `recovery` share of the total
        // supply (`recorded decision`: the spec only says "scaled_share").
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
    /// Macro zone from temperature using the **params** thresholds (`ECON-111` final: Fever >610,
    /// Hypothermia <600, Normal [600,610]) — always consistent with the thermostat.
    fn zone_at(&self, temp_mk: u64) -> MacroZone {
        if temp_mk > self.macro_state.params.fever_threshold_mk {
            MacroZone::Fever
        } else if temp_mk < self.macro_state.params.hypothermia_threshold_mk {
            MacroZone::Hypothermia
        } else {
            MacroZone::Normal
        }
    }

    /// Zone name for SVG/events (canonical strings `FEVER`/`HYPO`/`NORMAL`).
    fn zone_of(&self, temp_mk: u64) -> &'static str {
        self.zone_at(temp_mk).as_str()
    }

    /// `F7` / `P7` (`ADR-03`) — `nft_token(token_id)` → `NEP-171`/`NEP-177` view
    /// with `media = data:image/svg+xml;base64,...` carrying 8 real-time indicators
    /// + `role_mask` & `generation` (spike `S1.5`), red `#ff2255` when `Liquidated`.
    pub fn nft_token(&self, token_id: String) -> NftTokenView {
        let strain_id: StrainId = token_id
            .parse()
            .unwrap_or_else(|_| env::panic_str(constants::ERR_STRAIN_NOT_FOUND));
        let v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND);
        let now_sec = self.now_sec();
        self.nft_token_view_of(strain_id, v, now_sec)
    }

    /// Shared renderer behind the `nft_token` family (`nft_token`, `nft_tokens`,
    /// `nft_tokens_for_owner`): one source of truth for the `NEP-171`/`NEP-177` view.
    fn nft_token_view_of(&self, strain_id: StrainId, v: &StrainVessel, now_sec: u64) -> NftTokenView {
        let balance = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        let bounty =
            balance * constants::LIQUIDATOR_BOUNTY_BPS as u128 / constants::BPS_DENOMINATOR;
        let temp = self.macro_state.crucible_temp_at(now_sec * 1000);
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
            token_id: strain_id.to_string(),
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

    /// `NEP-177` — contract-level metadata (`nft_metadata`): the standard fields plus
    /// an inline base64 SVG logo (one visual identity with `ft_metadata`).
    pub fn nft_metadata(&self) -> NftContractMetadata {
        let logo = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect width=\"64\" height=\"64\" fill=\"#1b0f2e\"/><text x=\"32\" y=\"43\" font-family=\"monospace\" font-size=\"30\" fill=\"#ff8a3d\" text-anchor=\"middle\">S</text></svg>";
        NftContractMetadata {
            spec: "nft-1.0.0".to_string(),
            name: "Snacc".to_string(),
            symbol: "$SNACC".to_string(),
            icon: Some(format!(
                "data:image/svg+xml;base64,{}",
                svg::base64_encode(logo.as_bytes())
            )),
            base_uri: None,
            reference: None,
            reference_hash: None,
        }
    }

    /// `NEP-181` — total minted NFTs (`nft_total_supply`).
    pub fn nft_total_supply(&self) -> U128 {
        U128(self.next_strain_id.saturating_sub(1) as u128)
    }

    /// `NEP-181` — paginated enumeration (`nft_tokens`).
    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<NftTokenView> {
        let start = from_index.unwrap_or(U128(0)).0 as u64;
        let now_sec = self.now_sec();
        let total = self.next_strain_id.saturating_sub(1);
        let mut out = Vec::new();
        let mut i = start;
        while i < total && (limit.is_none() || (out.len() as u64) < limit.unwrap()) {
            i += 1;
            if let Some(v) = self.strains.get(&i) {
                out.push(self.nft_token_view_of(i, v, now_sec));
            }
        }
        out
    }

    /// `NEP-181` — per-owner supply (`nft_supply_for_owner`), exact scan of `strains`.
    pub fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        let mut count: u64 = 0;
        for id in 1..self.next_strain_id {
            if let Some(v) = self.strains.get(&id) {
                if v.owner_id == account_id {
                    count += 1;
                }
            }
        }
        U128(count as u128)
    }

    /// `NEP-181` — per-owner enumeration (`nft_tokens_for_owner`) with pagination.
    pub fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<NftTokenView> {
        let start = from_index.unwrap_or(U128(0)).0 as u64;
        let mut owned: Vec<StrainId> = (1..self.next_strain_id)
            .filter(|id| {
                self.strains
                    .get(id)
                    .map(|v| v.owner_id == account_id)
                    .unwrap_or(false)
            })
            .collect();
        owned.sort_unstable();
        let now_sec = self.now_sec();
        let mut out = Vec::new();
        for (_idx, id) in owned.iter().enumerate().skip(start as usize) {
            if let Some(cap) = limit {
                if (out.len() as u64) >= cap {
                    break;
                }
            }
            if let Some(v) = self.strains.get(id) {
                out.push(self.nft_token_view_of(*id, v, now_sec));
            }
        }
        out
    }
}

// =====================================================================
// N141-101..106 — surface the `NEP-141` standard for the `$SNACC` coin
// =====================================================================
#[near]
impl SnaccContract {
    /// Helper: the account's `NEP-141` balance (the `account_balances` ledger); 0 when unregistered.
    fn account_balance(&self, account: &AccountId) -> u128 {
        self.account_balances.get(account).copied().unwrap_or(0)
    }

    /// `N141-102` — recorded decision: `ft_balance_of` = `account_balances` ONLY
    /// (money inside a Strain can only move through game mechanics).
    pub fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.account_balance(&account_id))
    }

    /// `N141-102` — `ft_total_supply` = `macro_state.total_snacc_supply` (`I-01`).
    pub fn ft_total_supply(&self) -> U128 {
        U128(self.macro_state.total_snacc_supply)
    }

    /// `N141-102` — total wealth for display: `account_balances` + Σ strain balances
    /// owned by the account (the `strains_by_owner` index).
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

    /// `N141-101` — `ft_transfer` (`#[payable]`, exactly 1 yoctoNEAR required).
    /// The receiver must be storage-registered (`ERR_ACCOUNT_NOT_REGISTERED`); insufficient balance
    /// → `ERR_INSUFFICIENT_BALANCE`; self-transfer is neutral (debit then credit the same account).
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
        // `N141-105` — `amount` is a decimal string (u128) and `memo` is a
        // JSON string or `null` per the NEP-141 events standard.
        events::emit_standard_event(
            "nep141",
            "ft_transfer",
            &format!(
                r#"{{"old_owner_id":"{}","new_owner_id":"{}","amount":"{}","memo":{}}}"#,
                sender_id,
                receiver_id,
                amount.0,
                match memo {
                    Some(m) => near_sdk::serde_json::to_string(&m).unwrap(),
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

    /// `N141-101` — `ft_transfer_call`: transfer + a `receiver.ft_on_transfer` promise
    /// + a `#[private] on_transfer_call` callback that refunds the unused part.
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
        // `NEP-141` convention: the `ft_on_transfer` argument = `{sender_id, amount, msg}`.
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

    /// `ADR-24` — anti-spoofing callback: refund the unused part of `ft_on_transfer`
    /// to the sender; when the promise fails, refund everything.
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
            Err(_) => amount.0, // `ft_on_transfer` failed -> full refund
        };
        if unused > 0 {
            let bal = self.account_balance(&sender_id);
            self.set_account_balance(&sender_id, bal.saturating_add(unused));
            // The remaining funds are pulled back from the receiver (conservation restored).
            let recv_bal = self.account_balance(&receiver_id);
            self.set_account_balance(&receiver_id, recv_bal.saturating_sub(unused));
        }
        U128(unused)
    }

    /// `N141-103` — `NEP-141` metadata. `decimals: 24` = recorded decision.
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

    /// `N141-104` — `storage_deposit` MVP: account -> total deposit registry (milliNEAR).
    /// `registration_only = true` registers with exactly the minimum and refunds any
    /// excess (NEP-145 `registration_only` semantics).
    #[payable]
    pub fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        let target = account_id.unwrap_or_else(env::predecessor_account_id);
        let attached = env::attached_deposit().as_yoctonear();
        let min_yocto =
            NearToken::from_millinear(constants::STORAGE_DEPOSIT_MILLINEAR).as_yoctonear();
        let existing = self.registered_storage.get(&target).copied().unwrap_or(0);
        if registration_only.unwrap_or(false) {
            if existing > 0 {
                if attached > 0 {
                    let _ = Promise::new(target).transfer(NearToken::from_yoctonear(attached));
                }
                let total = existing;
                return StorageBalance {
                    total: U128(total),
                    available: U128(total),
                };
            }
            assert!(
                attached >= min_yocto,
                "{}",
                constants::ERR_INSUFFICIENT_DEPOSIT
            );
            let _ = self.registered_storage.insert(target.clone(), min_yocto);
            if attached > min_yocto {
                let _ =
                    Promise::new(target).transfer(NearToken::from_yoctonear(attached - min_yocto));
            }
            return StorageBalance {
                total: U128(min_yocto),
                available: U128(min_yocto),
            };
        }
        if existing == 0 {
            // The 5 milliNEAR minimum applies only to first registration (recorded decision).
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

    /// `N141-104` — `storage_withdraw`: withdraw the excess deposit. The minimum balance
    /// (`STORAGE_DEPOSIT_MILLINEAR`) stays locked while the account holds `$SNACC > 0`
    /// (recorded decision, MVP).
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
                // A full withdrawal = deregistration (cannot receive transfers anymore).
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

    /// `NEP-145` — storage balance bounds (`storage_balance_bounds`): the recorded
    /// first-registration minimum, no upper bound.
    pub fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        StorageBalanceBounds {
            min: U128(
                NearToken::from_millinear(constants::STORAGE_DEPOSIT_MILLINEAR).as_yoctonear(),
            ),
            max: None,
        }
    }

    /// `NEP-145` — `storage_unregister(force)`: removes the caller's registration and
    /// refunds the locked deposit (1 yoctoNEAR guard). With `$SNACC` still on the
    /// account the refund is refused unless `force = true` (recorded MVP rule).
    #[payable]
    pub fn storage_unregister(&mut self, force: Option<bool>) -> Option<StorageBalance> {
        Self::assert_one_yocto();
        let caller = env::predecessor_account_id();
        let total = self
            .registered_storage
            .get(&caller)
            .copied()
            .unwrap_or(0);
        if total == 0 {
            return None;
        }
        if self.account_balance(&caller) > 0 && !force.unwrap_or(false) {
            env::panic_str(constants::ERR_INSUFFICIENT_DEPOSIT);
        }
        let _ = self.registered_storage.remove(&caller);
        if total > 0 {
            let _ = Promise::new(caller).transfer(NearToken::from_yoctonear(total));
        }
        Some(StorageBalance {
            total: U128(total),
            available: U128(total),
        })
    }
}

/// `NEP-141` metadata (`N141-103`) — standard subset + inline SVG icon.
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

/// `N141-104` — MVP storage balance.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct StorageBalance {
    pub total: U128,
    pub available: U128,
}

/// `NEP-145` — storage balance bounds (`storage_balance_bounds`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct StorageBalanceBounds {
    pub min: U128,
    pub max: Option<U128>,
}

/// `NEP-177` — contract-level metadata (`nft_metadata`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct NftContractMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    pub base_uri: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

// =====================================================================
// DK-033 / DK-044 — Industrial Dividend Pool (`ADR-28`)
// =====================================================================
#[near]
impl SnaccContract {
    /// `DK-044` — burn `mt:elixir-aether` as a contribution to the running epoch's dividend
    /// (Phase 1 pro-rata; pays out only after the epoch closes).
    pub fn synthesize_and_redeem_aether(&mut self, strain_id: StrainId, amount: U128) {
        let caller = env::predecessor_account_id();
        let mut v = self
            .strains
            .get(&strain_id)
            .expect(constants::ERR_STRAIN_NOT_FOUND)
            .clone();
        // `V-01` — owner only.
        assert_eq!(caller, v.owner_id, "{}", constants::ERR_NOT_STRAIN_OWNER);
        // Caste gate: requires the `ROLE_SYNTHESIZER` bit.
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
        // Enough `mt:elixir-aether` stock.
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

    /// `DK-033` Phase 2 — claim the pro-rata dividend of a closed `(strain_id, epoch)`:
    /// `payout = contribution × epoch_dividend_rate[epoch] / DIVIDEND_RATE_SCALE`,
    /// capped at the pool balance (never mints — `DK-010`), once per `(epoch, strain)`.
    #[payable]
    pub fn claim_industrial_dividend(&mut self, strain_id: StrainId, epoch: Epoch) -> U128 {
        // Pipeline #7 — defensive payable (refund any attached deposit; see feed_strain).
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
        // The epoch must already be closed.
        assert!(
            epoch < self.macro_state.last_closed_epoch,
            "{}",
            constants::ERR_EPOCH_NOT_CLOSED
        );
        // Lookback claim limit `MAX_DIVIDEND_CLAIM_EPOCHS`.
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
        // Exact payout `contribution × inflow / max(burned, 1)` (256-bit, no overflow);
        // falls back to `rate` for epochs closed before totals were stored.
        let mut payout = match self.epoch_dividend_totals.get(&epoch).copied() {
            Some((inflow, burned)) => {
                mul_div_u128(contribution, inflow, std::cmp::max(burned, 1))
            }
            None => {
                let rate = self.epoch_dividend_rate.get(&epoch).copied().unwrap_or(0);
                mul_div_u128(contribution, rate, constants::DIVIDEND_RATE_SCALE)
            }
        };
        // Cap: payout ≤ pool balance (closes `V-04`); empty pool -> explicit error.
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
        // `ADR-29` — every balance mutation bumps the nonce.
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

    /// `DK-033` — the pool view: `(industrial_dividend_pool, epoch_dividend_inflow, last_closed_epoch)`.
    pub fn get_dividend_pool(&self) -> (U128, U128, Epoch) {
        (
            U128(self.macro_state.industrial_dividend_pool),
            U128(self.macro_state.epoch_dividend_inflow),
            self.macro_state.last_closed_epoch,
        )
    }

    /// `DK-033` — epoch rate (scaled by `DIVIDEND_RATE_SCALE`); 0 when not closed yet.
    pub fn get_epoch_dividend_rate(&self, epoch: Epoch) -> U128 {
        U128(self.epoch_dividend_rate.get(&epoch).copied().unwrap_or(0))
    }

    /// `DK-033` — `(epoch, strain)` contribution + claim status.
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
    /// `EN-101` — burst state view: `(active nutrient_burst_bps, nutrient_burst_until)`.
    /// The returned `bps` is the EFFECTIVE value for the running epoch (0 when
    /// expired — keeperless, `ADR-27`).
    pub fn get_burst_state(&self) -> (U128, Epoch) {
        let epoch = self.macro_state.current_epoch;
        (
            U128(u128::from(self.macro_state.active_burst_bps(epoch))),
            self.macro_state.nutrient_burst_until,
        )
    }

    /// `EN-103` — `feed_strain(strain_id, amount)`: burn `$SNACC` from the strain's own
    /// balance as food. Recorded decision (not nominalized in the spec):
    /// `base_fee = amount`; the portion that actually leaves the strain's balance is
    /// `effective_fee = base_fee × (10000 − burst) / 10000` while a burst is active, then
    /// enters the `F4` thermostat path (volume, Fever burn, `I-04` rebate, `DK-033` dividend split);
    /// the discount remainder stays in the strain's balance (this is where "cheaper feeding" lives).
    /// Metabolic recovery `+500` & `last_fed_sec = now` (the `F2` pattern).
    /// Further modifiers (order: burst → rescue → dormancy, all shrink the fee):
    /// - `EN-109` rescue feeding: a linear multiplier `RESCUE_MIN_FEE_BPS..=10000` over
    ///   `effective_vitality` — the closer to death, the cheaper (0 → 20%, 100% → full).
    /// - `EN-108` dormancy: when `decayed_volume < threshold` (`DORMANCY_THRESHOLD_BPS`,
    ///   recorded decision 1% of supply) upkeep drops to `UPKEEP_MIN_BPS = 50` bps.
    /// - `EN-105` the feed volume feeds the Great Feast accumulator + the top-3 leaderboard.
    #[payable]
    pub fn feed_strain(&mut self, strain_id: StrainId, amount: U128) -> StrainVesselView {
        // Pipeline #7 — defensive payable: any attached deposit is fully refunded
        // (a non-payable method traps the wasm without a message if an operator attaches a deposit).
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

        // `EN-103` — burst discount, capped by `EN-104` (5% Pantry/epoch, the `I-04` quota).
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

        // `EN-109` — rescue feeding: linear multiplier over `effective_vitality`.
        let rescue_bps = constants::RESCUE_MIN_FEE_BPS
            + (constants::BPS_DENOMINATOR - constants::RESCUE_MIN_FEE_BPS)
                * u128::from(eff)
                / u128::from(constants::VITALITY_CAP as u16);

        // `EN-108` — dormancy: upkeep drops to `UPKEEP_MIN_BPS` when volume is below the threshold.
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

        // DK-030 (triage of the EN-104 finding) — FEE FLOOR: the combination of burst × dormancy ×
        // rescue must not produce 0 (a free feed would slip past anti-farming).
        // The floor is 1 yocto: there is always a fee > 0. No other formula changes.
        let effective_fee = ((base_fee - discount) * rescue_bps / constants::BPS_DENOMINATOR
            * upkeep_bps
            / constants::BPS_DENOMINATOR)
            .max(1);

        // EN-117 (temperature is literal ATP) — atp_minted is computed from the effective fee BEFORE
        // the thermostat so `atp_minted × ATP_VOLUME_SCALE` lands in the same epoch's V_epoch.
        let atp_minted = Self::calc_atp_minted(
            effective_fee,
            v.efficiency_bps,
            v.organel.mitokondria,
        );
        let atp_volume = atp_minted.saturating_mul(constants::ATP_VOLUME_SCALE);

        // Debit `effective_fee` from the strain's balance → the `F4` thermostat path.
        self.set_strain_snacc(strain_id, bal - effective_fee);
        self.apply_thermostat_fee(strain_id, effective_fee, atp_volume);

        // `EN-105` — Great Feast accumulator + top-3 leaderboard (`EN-107`).
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

        // `EN-06`/§13a — ATP is minted from the effective fee (computed above, enters
        // V_epoch via `atp_volume` — EN-117 literal-ATP temperature).
        v.atp_balance = v.atp_balance.saturating_add(atp_minted);
        // Metabolic recovery (the `F2` pattern) + nonce.
        Self::recover_metabolism(&mut v, eff, now_sec);
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        events::emit_event_json(&format!(
            r#"{{"kind":"fed","strain_id":{},"base_fee":{},"effective_fee":{},"burst_bps":{},"rescue_bps":{},"dormant":{},"atp_minted":{}}}"#,
            strain_id, base_fee, effective_fee, burst_bps, rescue_bps, dormant, atp_minted
        ));
        self.get_strain(strain_id)
    }

    /// `EN-107` — top-3 feeder leaderboard maintenance (capped at 3 entries, gas-safe).
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

    /// `EN-107` — top-3 feeder leaderboard for one epoch (at most 3 entries).
    pub fn get_top_feeder(&self, epoch: Epoch) -> Vec<FeederEntry> {
        self.epoch_top_feeders.get(&epoch).cloned().unwrap_or_default()
    }

    /// `EN-112` — `get_appetite_meter()`: macro zone, temperature, burst countdown,
    /// dormancy status, and Great Feast progress (`EN-05` spec view).
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

/// `EN-112` — the `get_appetite_meter()` view.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct AppetiteMeterView {
    /// Macro zone (`NORMAL`/`FEVER`/`HYPO`).
    pub zone: String,
    pub crucible_temp: u64,
    pub current_epoch: Epoch,
    /// Active burst discount of the running epoch (0 when expired).
    pub burst_bps: u16,
    pub burst_until: Epoch,
    /// Burst countdown in epochs (`burst_until − current_epoch`).
    pub burst_epochs_left: Epoch,
    /// `EN-108` — dormancy status (volume < threshold).
    pub dormant: bool,
    /// `EN-105` — Great Feast progress of the running epoch.
    pub feast_volume: u128,
    pub feast_target: u128,
    /// `EN-125` — `SpeculativeFlux` (0..10000; betting intensity of the running epoch).
    pub risk_heat: u64,
}


// =====================================================================
// State migration — MUST be called once after redeploy (positional borsh)
// =====================================================================
#[near]
impl SnaccContract {
    /// NEAR state migration (standard pattern): `#[init(ignore_state)]` runs WITHOUT
    /// loading the old state. All collections use the same byte prefixes as
    /// `new()`, so the old trie data (living in prefixed storage keys) stays
    /// readable — only the state blob (borsh) is replaced with the new layout.
    /// Without this, redeploying to an account with old state would fail deserialization
    /// (new fields in the middle of the layout) = every view/call panics.
    /// SEC-112 — migrate with PREFIX GENERATION: the `__snacc_prefix_gen_v1` marker
    /// (a raw storage key outside all collections) is read → gen+1 → ALL collections
    /// use a fresh `[gen, base]` prefix → old data (old prefix) becomes unreadable.
    /// `#[init(ignore_state)]` skips deserializing the old struct (layout changes are safe).
    /// Old data becomes unreachable garbage (storage rent cost, not a logic leak).
    #[init(ignore_state)]
    #[private]
    pub fn migrate() -> Self {
        // SEC-113 — `#[private]`: only the contract's own account may trigger
        // migrate (any other caller → the entire state becomes unreadable + owner reset).
        let old_gen = env::storage_read(GEN_MARKER)
            .and_then(|b| b.first().copied())
            .unwrap_or(0);
        let gen = old_gen.saturating_add(1);
        env::storage_write(GEN_MARKER, &[gen]);
        let mut c = Self::build(gen);
        // SEC-113 — the old owner is preserved via the raw-storage marker; fallback:
        // the private caller (= the contract's own account).
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
// Pipeline #3 — EN-121/122/123 (colony, cytokinesis, adoption) + EN-125 + NPC ADR-30
// =====================================================================
#[near]
impl SnaccContract {
    /// Count the account's ACTIVE strains (colony; `Liquidated`/`SplicedBurned`
    /// are not counted — the `MAX_COLONY_SIZE` fence applies to living cells).
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

    /// `EN-121` — one account's colony (living cells, ≤ `MAX_COLONY_SIZE` by construction).
    /// TODO(main-session): colony Σ ATP = Σ `atp_balance` (the EN-114 field) — the
    /// `get_colony_atp(owner_id)` view is added once the ATP field merges (EN-121:
    /// "the colony's shared $ATP budget").
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

    /// `EN-121` — Σ ATP of one account's active colony (the shared `$ATP` budget, LC-20).
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

    /// `EN-122` — `divide_strain`: cytokinesis (§13a). The child's balance, vitality, and
    /// `mt:spore-ignis` spores = half (the parent is cut — Σ parent+child = parent balance);
    /// organelles ÷2 (child floors, parent keeps the remainder — totals preserved); the child's `generation` = parent+1;
    /// ATP cost 3; fences `MAX_COLONY_SIZE=5` / `MAX_GENERATION=8`.
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
        // Colony/generation fences (`EN-124`/§13a).
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
        // The parent must be alive (children are not born from dead cells).
        let now_sec = self.now_sec();
        let _ = Self::assert_not_starving(&parent, now_sec);
        // `EN-116`/§13a — ATP cost of cytokinesis = 3.
        Self::pay_atp(&mut parent, 3);
        // `EN-124` — protective demographics invariant: division (free reproduction)
        // must have a "budget" from mergers + lyses. Recorded interpretation: the literal
        // form `births ≤ merges + lyses` makes the first incubation impossible
        // (births=1, merges+lyses=0), so the gate is applied to divides:
        // `total_divides ≤ total_merges + total_lyses` (paid incubation is free —
        // real capital enters, `V-04` stays safe). Reported in the pipeline #5 report.
        self.macro_state.total_divides = self.macro_state.total_divides.saturating_add(1);
        // births = incubation + divide (§13a/brief).
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

        // Cash split: child = floor(bal/2), parent = the remainder (Σ = parent balance, `I-01`).
        let bal = self.snacc_balances.get(&strain_id).copied().unwrap_or(0);
        let child_bal = bal / 2;
        self.set_strain_snacc(strain_id, bal - child_bal);
        // Vitality split: the child takes half, the parent is cut.
        let child_vitality = parent.vitality / 2;
        parent.vitality -= child_vitality;
        // `mt:spore-ignis` spore split: child = floor(n/2), parent = the remainder.
        let ignis = self.inv_balance(strain_id, constants::GENESIS_SPORE_TOKEN_ID);
        let child_spores = ignis / 2;
        let _ = self
            .inventories
            .insert((strain_id, constants::GENESIS_SPORE_TOKEN_ID.to_string()), ignis - child_spores);

        // Organelles ÷2 (child floors per slot, parent keeps the remainder — totals preserved).
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

        // Child: genome (role_mask, efficiency, phenotype) & bmr are inherited; gen = parent+1.
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
            // §13a — child ATP = `INITIAL_ATP` (50, birth energy).
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
        // Colony index: the child joins the owner's list.
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

    /// `EN-123` — `adopt`: take over a Distressed Strain (`1 <= eff <= 2000`) at
    /// `book_value` **WITHOUT the 15% premium** → 100% goes to the old owner; ownership moves
    /// to the raider's owner (prevents lysis — competes with `hostile_takeover_splice`).
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
        // `SEC-110`/`I-14` — anti seller-drain: the target's nonce must match (the hostile pattern).
        target.assert_atomic_ready(expected_target_nonce);
        let now_sec = self.now_sec();
        let eff = target.effective_vitality(now_sec);
        // Distressed window (Tier 2); eff 0 = Tier 3 → the F6 path (lysis), not adopt.
        assert!((1..=2000).contains(&eff), "{}", constants::ERR_NOT_DISTRESSED);
        // The adopter's colony must have room (a living cell moves colonies).
        assert!(
            self.colony_active_count(&caller) < u64::from(constants::MAX_COLONY_SIZE),
            "{}",
            constants::ERR_COLONY_FULL
        );
        // Book value (balance + Σ inventory × floor_price 1) WITHOUT premium → 100% to the old owner.
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

        // Ownership moves: update the `strains_by_owner` index (leave the old colony).
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

    /// `LC-20` — `conjugate_traits`: plasmid conjugation — **copy** the `phenotype_trait`
    /// from the source strain to the target strain WITHOUT a merger (the source keeps its own).
    /// Conditions (recorded decision): same owner (`ERR_NOT_SAME_OWNER`), both cells
    /// alive, the `phenotype_trait` parameter must match the source's current trait
    /// (preventing a TOCTOU surprise — `ERR_TRAIT_MISMATCH`); the target's ATP cost = 2
    /// (decision: §13a does not list conjugation, so it follows the transmute cost).
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
        // ATP cost of conjugation = 2, paid by the target strain (recorded decision).
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

    /// `EN-125` — `SpeculativeFlux` surfaced: a per-epoch "betting intensity" indicator
    /// (scale 0..10000). Definition (recorded decision — not nominalized in the vault):
    /// base = Fever proximity (`temp × 10000 / 1400`), +1000 while a Nutrient Burst
    /// is active (the EN-01 betting moment), +1000 when the dividend pool ≥ 1% of supply (the
    /// DK-033 claim moment). Capped at 10000.
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

    /// `ADR-30` — view of one NPC.
    pub fn get_npc(&self, npc_id: String) -> NpcMerchantAgent {
        self.npc_agents
            .get(&npc_id)
            .cloned()
            .expect(constants::ERR_NPC_UNKNOWN)
    }

    /// NPC spore price quote (pure; used by quote & buy):
    /// `unit = max(1, floor(P_base × S_target / max(S_after, 1)))` (`I-15` — the price
    /// rises when the NPC's stock shrinks below target; equality satisfies "≥ floor").
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

    /// `ADR-30` — quote for buying spores from an NPC (view; `now_sec` from spec §5.1 is not
    /// taken as a parameter — `V-02`, consistent with the `hostile_takeover_splice` decision).
    pub fn npc_quote_spores(&self, npc_id: String, buy_amount: u64) -> NpcQuoteView {
        assert!(buy_amount > 0, "{}", constants::ERR_BAD_AMOUNT);
        let npc = self
            .npc_agents
            .get(&npc_id)
            .cloned()
            .expect(constants::ERR_NPC_UNKNOWN);
        self.npc_quote_internal(&npc, buy_amount)
    }

    /// `ADR-30` / `I-15` — atomically buy `mt:spore-ignis` spores from an NPC using the
    /// strain's balance as cash. Exact conservation: `Δnpc.snacc_reserve == −Δstrain.balance ==
    /// total_cost` and `Δnpc.spores_stock == −Δstrain inventory == buy_amount`.
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
        // `I-15` conservation — both sides exactly equal (+ the `sum_npc_reserve` aggregate).
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
// Pipeline #4 — EN-114..119: ATP/mitochondria metabolism + ECON-111 calibration
// =====================================================================
#[near]
impl SnaccContract {
    /// `EN-118`/§13a — `allocate_atp`: allocate ATP to organel slots.
    /// 1× per epoch (`ERR_ALREADY_ALLOCATED`); level ≤ `MAX_ORGANEL_LEVEL = 5`
    /// (`ERR_ORGANEL_LEVEL_MAX`); pay 1 ATP per level INCREASE; set the organelles,
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
        // Cost: 1 ATP per level increase per slot (§13a "allocate 1/level").
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
// Pipeline #5 — LC-16: whole-cell transplantation (`NEP-171` nft_transfer)
// =====================================================================

/// `EN-126` — gas reserved for the `nft_resolve_transfer` callback (lock release,
/// optional revert with the colony index move, event).
const GAS_FOR_RESOLVE_TRANSFER: near_sdk::Gas = near_sdk::Gas::from_tgas(20);
/// `EN-126` — gas `nft_transfer_call` keeps back from the receiver: its own execution
/// (transfer core + promise creation) plus `GAS_FOR_RESOLVE_TRANSFER`. The receiver's
/// `nft_on_transfer` gets `prepaid_gas − GAS_FOR_NFT_TRANSFER_CALL`.
const GAS_FOR_NFT_TRANSFER_CALL: near_sdk::Gas =
    near_sdk::Gas::from_tgas(30 + GAS_FOR_RESOLVE_TRANSFER.as_tgas());

#[near]
impl SnaccContract {
    /// Move a colony index entry: out of the `from` list, into the `to` list.
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

    /// The `nep171` `nft_transfer` NEP-297 event — one shape for every ownership move
    /// (`nft_transfer`, `nft_transfer_call`, and the `nft_resolve_transfer` revert).
    fn emit_nft_transfer_event(
        old_owner_id: &AccountId,
        new_owner_id: &AccountId,
        strain_id: StrainId,
        memo: Option<&str>,
    ) {
        events::emit_standard_event(
            "nep171",
            "nft_transfer",
            &format!(
                r#"{{"old_owner_id":"{}","new_owner_id":"{}","token_ids":["{}"],"approved_account_ids":null,"memo":{}}}"#,
                old_owner_id,
                new_owner_id,
                strain_id,
                match memo {
                    Some(m) => near_sdk::serde_json::to_string(m).unwrap(),
                    None => "null".to_string(),
                }
            ),
        );
    }

    /// `LC-16` / `EN-126` — the shared transfer core behind `nft_transfer` and
    /// `nft_transfer_call`, so both apply exactly the same Snacc rules: `sender_id` must
    /// own the strain (`ERR_NOT_OWNED_BY_CALLEE`), no self-transfer (`ERR_SAME_STRAIN`),
    /// dead cells cannot move (`ERR_ALREADY_LIQUIDATED`), `state_nonce` is bumped, the
    /// colony index moves on both sides, and the `nep171` event is emitted.
    /// Returns `(strain_id, previous_owner_id)`.
    fn internal_nft_transfer(
        &mut self,
        sender_id: &AccountId,
        receiver_id: &AccountId,
        token_id: &str,
        memo: Option<&str>,
    ) -> (StrainId, AccountId) {
        assert_ne!(receiver_id, sender_id, "{}", constants::ERR_SAME_STRAIN);
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
            sender_id,
            &old_owner,
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
        self.move_strain_owner_index(strain_id, &old_owner, receiver_id);
        // NEP-297 event with standard `nep171` — the exact NEP-171 standard shape.
        Self::emit_nft_transfer_event(&old_owner, receiver_id, strain_id, memo);
        (strain_id, old_owner)
    }

    /// `LC-16` — standard `NEP-171` `nft_transfer`: whole-cell transplantation — the
    /// `$SNACC` balance (stomach), spores/inventory, organelles, ATP, and `state_nonce` follow
    /// the vessel (all state is keyed by `strain_id`). Exactly 1 yoctoNEAR required.
    /// `approval_id` is accepted for NEP-171 ABI shape (`number|null`); the `NEP-178`
    /// approval registry is not implemented (recorded decision — the parameter is ignored).
    /// Dead cells (`Liquidated`/`SplicedBurned`) cannot be transferred.
    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        // NEP-178 (approval management) is not implemented yet — the parameter is accepted
        // for NEP-171 ABI shape and ignored (recorded decision).
        let _ = approval_id;
        Self::assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let _ = self.internal_nft_transfer(&sender_id, &receiver_id, &token_id, memo.as_deref());
    }

    /// `LC-16` / `EN-126` — standard NEP-171 `nft_transfer_call`: the same transfer core
    /// as `nft_transfer` (every Snacc rule applies identically), then
    /// `receiver_id.nft_on_transfer(sender_id, previous_owner_id, token_id, msg)` with all
    /// prepaid gas except `GAS_FOR_NFT_TRANSFER_CALL`, then the `#[private]`
    /// `nft_resolve_transfer` callback (`GAS_FOR_RESOLVE_TRANSFER` reserved). Exactly
    /// 1 yoctoNEAR required. Resolves to `false` only when the token was returned.
    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        // NEP-178 is not implemented — `approval_id` is accepted for ABI shape and ignored.
        let _ = approval_id;
        Self::assert_one_yocto();
        assert!(
            env::prepaid_gas() > GAS_FOR_NFT_TRANSFER_CALL,
            "{}",
            constants::ERR_MORE_GAS_REQUIRED
        );
        let sender_id = env::predecessor_account_id();
        let (strain_id, previous_owner_id) =
            self.internal_nft_transfer(&sender_id, &receiver_id, &token_id, memo.as_deref());
        // SEC-111 (triage Option a) — lock `pending_callbacks` during the promise window;
        // released in `nft_resolve_transfer` (all paths). A counter, so a receiver that
        // forwards the token with another `nft_transfer_call` does not unlock it early.
        if let Some(mut v) = self.strains.get(&strain_id).cloned() {
            v.pending_callbacks = v.pending_callbacks.saturating_add(1);
            v.record_mutation();
            let _ = self.strains.insert(strain_id, v);
        }
        let token_id = strain_id.to_string();
        let args = near_sdk::serde_json::to_vec(&near_sdk::serde_json::json!({
            "sender_id": sender_id,
            "previous_owner_id": previous_owner_id,
            "token_id": token_id,
            "msg": msg,
        }))
        .unwrap_or_else(|_| env::abort());
        Promise::new(receiver_id.clone())
            .function_call(
                "nft_on_transfer".to_string(),
                args,
                NearToken::from_yoctonear(0),
                env::prepaid_gas().saturating_sub(GAS_FOR_NFT_TRANSFER_CALL),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_RESOLVE_TRANSFER)
                    .nft_resolve_transfer(previous_owner_id, receiver_id, token_id, None),
            )
            .into()
    }

    /// `ADR-24` / `EN-126` — `nft_transfer_call` resolution (NEP-171 core). The receiver's
    /// `nft_on_transfer` returns a `bool` (`true` = give the token back); a failed promise
    /// or a non-`bool` result also counts as "give back". The token is returned to
    /// `previous_owner_id` ONLY while it is still owned by `receiver_id` and still alive
    /// (a receiver that already moved it on, or a cell that died in the window, is left
    /// as is). Returns `false` only when the token was returned to `previous_owner_id`;
    /// otherwise `true` (the transfer to `receiver_id` stands, NEP-171).
    /// `approved_account_ids` is always `null` (no `NEP-178` registry).
    #[private]
    pub fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: String,
        approved_account_ids: Option<std::collections::HashMap<AccountId, u64>>,
    ) -> bool {
        let _ = approved_account_ids;
        let Ok(strain_id) = token_id.parse::<StrainId>() else {
            return true;
        };
        // SEC-111 — release the `pending_callbacks` lock on ALL paths (accepted/rejected).
        let Some(mut v) = self.strains.get(&strain_id).cloned() else {
            return true;
        };
        v.pending_callbacks = v.pending_callbacks.saturating_sub(1);
        v.record_mutation();
        // A bool is at most 5 bytes; a longer result is not a valid answer → give back.
        let must_revert = match env::promise_result_checked(0, 16) {
            Ok(bytes) => near_sdk::serde_json::from_slice::<bool>(&bytes).unwrap_or(true),
            Err(_) => true,
        };
        let still_with_receiver = v.owner_id == receiver_id
            && !matches!(
                v.status,
                StrainStatus::Liquidated | StrainStatus::SplicedBurned
            );
        if !must_revert || !still_with_receiver {
            let _ = self.strains.insert(strain_id, v);
            // NEP-171 (same as `near-contract-standards`): the transfer to `receiver_id`
            // stands — `true`. Only an actual revert returns `false`.
            return true;
        }
        v.owner_id = previous_owner_id.clone();
        v.record_mutation();
        let _ = self.strains.insert(strain_id, v);
        self.move_strain_owner_index(strain_id, &receiver_id, &previous_owner_id);
        Self::emit_nft_transfer_event(
            &receiver_id,
            &previous_owner_id,
            strain_id,
            Some("nft_transfer_call rejected"),
        );
        false
    }
}

// =====================================================================
// Pipeline #6 — missing views (KPI audit findings J3.8/J3.9):
// `get_inventory` (NEP-245) & `get_global_solvency` (the `V-04` invariant)
// =====================================================================
#[near]
impl SnaccContract {
    /// J3.8 — the balance of one `NEP-245` molecule on a strain. `0` when absent —
    /// does NOT panic (unknown strain/token = zero balance, recorded decision).
    pub fn get_inventory(&self, strain_id: StrainId, token_id: String) -> U128 {
        U128(self.inv_balance(strain_id, &token_id))
    }

    /// J3.9 — the full `V-04` invariant (including the account & NPC ledgers).
    /// Exact formula (recorded decision; all integers — tolerance = 0):
    ///   income = `total_minted (= supply + burned) + total_catalyst_fees`
    ///   spending = `total_burned + treasury + industrial_dividend_pool
    ///                + epoch_dividend_inflow + Σstrain balances + Σaccount balances + Σnpc_reserve`
    /// `drift = income − spending`; `solvent = (drift == 0)`.
    /// `drift ≠ 0` = a bookkeeping bug (reported honestly, not hidden).
    /// Exclusions (separate ledgers, not `$SNACC`): the NEP-141 storage deposit,
    /// the `materialize_subaccount` deposit (real NEAR to the sub-account).
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

/// J3.9 — the `V-04` invariant view (all components + signed drift).
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
    /// `income − spending`; `0` = solvent.
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
mod qa_nep_tests;

#[cfg(test)]
mod qa_tee_tests;

#[cfg(test)]
mod qa_en124_tests;

#[cfg(test)]
mod qa_diet_tests;

#[cfg(test)]
mod qa_audit_tests;

#[cfg(test)]
mod qa_regression_tests;

#[cfg(test)]
mod qa_f1_tests;

#[cfg(test)]
mod qa_f2_tests;

#[cfg(test)]
mod qa_f6_tests;

#[cfg(test)]
mod qa_div_tests;

#[cfg(test)]
mod qa_macro_tests;

#[cfg(test)]
mod qa_svg_tests;

#[cfg(test)]
mod qa_npc_tests;

#[cfg(test)]
mod qa_en126_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::serde_json;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    /// The official near-sdk 5 test context (`testing_env!` + `VMContextBuilder`).
    fn set_context(predecessor: &str, deposit: NearToken) {
        set_context_at(predecessor, deposit, 3_600_000_000_000); // 1 hour — Lazy BMR kept in check
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

    /// Pipeline #7 — the sub-account suffix is derived from the contract account (regression for the
    /// `VAULT_SUBACCOUNT_SUFFIX` bug that hardcoded `.snacc-v001.near` and failed on testnet).
    #[test]
    fn pipeline7_suffix_follows_contract_account() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(
            c.get_strain(1).vault_subaccount.to_string(),
            "s-1-0.snacc-v001.testnet"
        );
        // A contract on another account -> the suffix follows (the parent always exists).
        let mut b = VMContextBuilder::new();
        b.current_account_id("snacc-lab.testnet".parse().unwrap())
            .predecessor_account_id(ALKEMIS_ID.parse().unwrap())
            .attached_deposit(NearToken::from_yoctonear(10_000))
            .block_timestamp(3_600_000_000_000);
        testing_env!(b.build());
        let v = c.incubate_strain(state::StrainRole::Extractor, U128(10_000), 0);
        assert_eq!(v.vault_subaccount.to_string(), "s-2-0.snacc-lab.testnet");
    }

    /// Pipeline #7 — `feed_strain` is now payable: an attached deposit does NOT trap;
    /// calling with a deposit succeeds (refund via promise).
    #[test]
    fn pipeline7_feed_payable_with_deposit() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000_000)); // attached deposit
        let v = c.feed_strain(1, U128(1_000)); // previously: non-payable trap
        assert_eq!(v.efficiency_bps, 10000);
    }

    /// Pipeline #7 — `claim_industrial_dividend` is now payable too (the same regression).
    #[test]
    fn pipeline7_claim_payable_with_deposit() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(500_000)); // attached deposit
        c.synthesize_and_redeem_aether(2, U128(1));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000_000)); // attached deposit
        let payout = c.claim_industrial_dividend(2, 0); // previously: non-payable trap
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

    // Helper: call incubate_strain with attached deposit == stake.
    fn call_incubate(c: &mut SnaccContract, stake: u128, role_name: &str, spores: u64) -> state::StrainVesselView {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(stake));
        c.incubate_strain(role(role_name), U128(stake), spores)
    }

    /// (a) exact 80/20 split: stake=1 NEAR -> strain=8e11, treasury=2e11.
    #[test]
    fn vc003_split_80_20_exact() {
        let mut c = SnaccContract::new();
        let stake = 1_000_000_000_000u128; // 1e12 yocto = 1 NEAR
        let view = call_incubate(&mut c, stake, EXTRACTOR_ROLE, 5);
        assert_eq!(view.strain_id, 1);
        assert_eq!(c.snacc_balances.get(&1), Some(&800_000_000_000));
        assert_eq!(c.treasury, 200_000_000_000);
    }

    /// (a) argument 0: the amount is taken from the attached deposit.
    #[test]
    fn vc003_argument_0_takes_attached_deposit() {
        let mut c = SnaccContract::new();
        let stake = 500_000_000_000u128;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(stake));
        let view = c.incubate_strain(role(EXTRACTOR_ROLE), U128(0), 3);
        assert_eq!(view.strain_id, 1);
        assert_eq!(c.snacc_balances.get(&1), Some(&400_000_000_000)); // 80%
        assert_eq!(c.treasury, 100_000_000_000); // 20%
    }

    /// (a) deposit < the argument's amount -> rejected.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn vc003_deposit_mismatch_rejected() {
        let mut c = SnaccContract::new();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(100));
        let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(1_000_000_000_000), 5);
    }

    /// (b) BMR per caste: Extractor=100, Fermenter=120, Synthesizer=150.
    #[test]
    fn vc003_bmr_per_caste() {
        let mut c = SnaccContract::new();
        let e = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(e.bmr_per_hour, 100);
        let f = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        assert_eq!(f.bmr_per_hour, 120);
        let s = call_incubate(&mut c, 1_000_000_000_000u128, SYNTHESIZER_ROLE, 0);
        assert_eq!(s.bmr_per_hour, 150);
    }

    /// (c) vitality=8500 & efficiency=10000 on all castes.
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
        // Pipeline #7: suffix = the contract account (`.testnet` on testnet).
        assert_eq!(v1.vault_subaccount.to_string(), "s-1-0.snacc-v001.testnet");
        let v2 = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        assert_eq!(v2.vault_subaccount.to_string(), "s-2-0.snacc-v001.testnet");
    }

    /// (e) spores are minted into the NEP-245 inventory (strain_id, token_id).
    #[test]
    fn vc003_spores_minted() {
        let mut c = SnaccContract::new();
        call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 7);
        let key = (1u64, "mt:spore-ignis".to_string());
        assert_eq!(c.inventories.get(&key), Some(&7));
        assert_eq!(state::constants::GENESIS_SPORE_TOKEN_ID, "mt:spore-ignis");
    }

    /// (f) get_strain JSON round-trip — `StrainVesselView` JSON deserialization.
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

    /// (g) deposit < 0.02 NEAR is rejected on materialize_subaccount.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn vc003_materialize_insufficient_deposit_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.materialize_subaccount(1);
    }

    /// (h) non-owner is rejected on materialize_subaccount.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn vc003_non_owner_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_near(1));
        let _ = c.materialize_subaccount(1);
    }

    /// (e) strain_id is sequential starting at 1.
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

    /// (e) AlkemisProfile: joined_sec at first, strain_count += 1.
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

    /// (e) StrainRole has only 3 variants, each a single bit.
    #[test]
    fn vc003_strain_role_enum() {
        use state::StrainRole;
        assert_eq!(StrainRole::Extractor.to_role_mask(), 0b001);
        assert_eq!(StrainRole::Fermenter.to_role_mask(), 0b010);
        assert_eq!(StrainRole::Synthesizer.to_role_mask(), 0b100);
    }

    /// get_strain with an unregistered strain_id -> rejected.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn vc003_get_strain_not_found() {
        let c = SnaccContract::new();
        let _ = c.get_strain(42);
    }

    /// (a) argument 0 without deposit -> rejected.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn vc003_argument_0_without_deposit() {
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

    /// (a) Lazy BMR: vitality exhausted -> `ERR_STRAIN_STARVING`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn vc004_starving_gate_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        // 200 hours × BMR 100/h = 20000 > vitality 8500 -> effective 0.
        set_context_at(
            ALKEMIS_ID,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
    }

    /// (b) role gate: Extractor may not transmute.
    #[test]
    #[should_panic(expected = "ERR_ROLE_NOT_ALLOWED")]
    fn vc004_role_gate_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
    }

    /// (c) extract mints 1 spore of the given type.
    #[test]
    fn vc004_extract_mint_by_type() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 3); // genesis 3 ignis
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(7));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(7));
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&4));
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-cryo".to_string(), U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:spore-cryo".to_string())), Some(&1));
        // baseline treasury = 20% of the incubation stake (2e11) + catalyst 7: fee 7 lands in the
        // Hypothermia zone (temp=0); DK-033 inflow = 7×5000/10000 = 3, treasury = 7−3 = 4;
        // rebate 7×200/10000 = 0.
        assert_eq!(c.treasury, 200_000_000_000 + 4);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 3);
    }

    #[test]
    #[should_panic(expected = "ERR_UNKNOWN_TOKEN")]
    fn vc004_extract_unknown_token_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:bogus".to_string(), U128(0));
    }

    /// (d) transmute burn -> mint × efficiency_bps (10000 = 1:1; 5000 = 0.5×).
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
        // change efficiency to 5000 -> output 0.5×
        let mut vessel = c.strains.get(&1).unwrap().clone();
        vessel.efficiency_bps = 5000;
        let _ = c.strains.insert(1, vessel);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 4, U128(0));
        assert_eq!(c.inventories.get(&(1, "mt:enzyme-amalgam".to_string())), Some(&6));
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&2));
    }

    /// (e) enzyme recipes: redactase (ignis+lumin) & synthase (cryo).
    #[test]
    fn vc004_transmute_recipes_redactase_synthase() {
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

    /// (e) recipe not met (odd amount) -> `ERR_RECIPE_NOT_MET`.
    #[test]
    #[should_panic(expected = "ERR_RECIPE_NOT_MET")]
    fn vc004_transmute_recipe_not_met() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 10);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 3, U128(0));
    }

    /// (h) insufficient stock -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn vc004_insufficient_spore_stock_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 4, U128(0));
    }

    /// (e) elixirs: homunculus (2 amalgam + 1 synthase) & homeostasis (1+1) per batch.
    #[test]
    fn vc004_synthesize_elixir_recipes() {
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

    /// (f) aether requires TEE -> `ERR_REQUIRES_TEE_ATTESTATION`.
    #[test]
    #[should_panic(expected = "ERR_REQUIRES_TEE_ATTESTATION")]
    fn vc004_aether_rejected_without_tee() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, SYNTHESIZER_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.synthesize_elixir(1, "mt:elixir-aether".to_string(), 3, U128(0));
    }

    /// (g) metabolic recovery: vitality +500 & last_fed reset.
    #[test]
    fn vc004_metabolic_recovery() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
        // effective 8500 (1 hour has not yet decremented a full point), +500 -> 9000.
        assert_eq!(v.vitality, 9000);
        assert_eq!(v.last_fed_sec, 3600);
    }

    /// catalyst: deposit != amount -> `ERR_STAKE_DEPOSIT_MISMATCH`.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn vc004_catalyst_mismatch_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(5));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(9));
    }

    /// `migrate()` — the NEAR state migration pattern; called once after redeploy.
    #[test]
    fn migrate_builds_new_state() {
        let c = SnaccContract::migrate();
        assert_eq!(c.protocol_version(), "snacc-v001");
        assert_eq!(c.ft_total_supply(), U128(0));
    }

    // ----------------------------------------------------------------------
    // VC-005 — F4: Macro Thermostat CrucibleTemp (ADR-04, I-01, I-04)
    // ----------------------------------------------------------------------

    /// (1 & 2) The `get_crucible_temp` temperature formula including lazy decay at 2 time points (`ADR-04`).
    #[test]
    fn vc005_crucible_temp_lazy_decay_two_time_points() {
        let mut c = SnaccContract::new();
        // stake = 10_000 -> total_snacc_supply = 10_000
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(c.macro_state.total_snacc_supply, 10_000);

        // Time point 1: t1 = 3600s, fee = 10_000 + the K3 leg (1 spore × 5 × 100 = 500)
        // -> volume_epoch = 10_500; temp = floor(1000×10_500/10_000) = 1050 > 610 (Fever!)
        // -> burn = 350 bps × 10_000 = 350 -> supply = 9_650.
        let t1_ns = 3_600_000_000_000u64;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(10_000), t1_ns);
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.total_snacc_supply, 9_650);
        assert_eq!(c.get_crucible_temp(), 1088); // floor(1000×10_500/9_650)

        // Time point 2: t2 = t1 + 40 s -> decay = 2_000 -> V_decayed = 8_500
        // temp(t2) = floor(1000 × 8_500 / 9_650) = 880 mK (still Fever > 610)
        let t2_ns = t1_ns + 40 * 1_000_000_000;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(0), t2_ns);
        assert_eq!(c.get_crucible_temp(), 880);

        // After 500 s (decay 25_000 > 10_500) -> saturates to 0
        let t3_ns = t1_ns + 500 * 1_000_000_000;
        set_context_at(ALKEMIS_ID, NearToken::from_yoctonear(0), t3_ns);
        assert_eq!(c.get_crucible_temp(), 0);
    }

    /// (3) Calcination Fever (`temp > 1400`): burns 350 bps + the `I-01` invariant
    /// (`total_snacc_supply` drops by `burn`, `total_snacc_burned` rises by the same, the rest to the treasury).
    #[test]
    fn vc005_fever_burn_350_bps_and_invariant_i01() {
        let mut c = SnaccContract::new();
        let stake = 10_000u128; // treasury awal = 2_000, strain = 8_000, supply = 10_000
        let _ = call_incubate(&mut c, stake, EXTRACTOR_ROLE, 0);
        let supply_before = c.macro_state.total_snacc_supply;
        let treasury_before = c.treasury;

        // Fee = 20_000 -> volume_epoch = 20_000 -> temp = floor(1000 * 20_000 / 10_000) = 2000 > 1400 (Fever!)
        // burn = 20_000 * 350 / 10_000 = 700; the rest to the treasury = 19_300.
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
        // `DK-033`: the non-burn remainder 19_300 is split 50/50 — inflow 9_650, treasury 9_650.
        assert_eq!(c.treasury, treasury_before + 9_650);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 9_650);
    }

    /// (3) Hypothermia (`temp < 600`): a 200 bps rebate from the treasury to `snacc_balances[strain_id]`.
    #[test]
    fn vc005_hypothermia_rebate_200_bps() {
        let mut c = SnaccContract::new();
        let stake = 1_000_000u128; // supply = 1_000_000, treasury = 200_000, strain = 800_000
        let _ = call_incubate(&mut c, stake, EXTRACTOR_ROLE, 0);
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch(); // snapshot epoch_treasury_initial = 200_000 (the 500 bps quota = 10_000)

        let bal_before = c.snacc_balances.get(&1).copied().unwrap();
        let treasury_before = c.treasury;

        // Fee = 10_000 -> temp = floor(1000 * 10_000 / 1_000_000) = 10 < 600 (Hypothermia)
        // rebate = 10_000 * 200 / 10_000 = 200 (from the treasury to snacc_balances[1]).
        let fee = 10_000u128;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(fee));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(fee));

        let expected_rebate = 200u128;
        assert_eq!(
            c.snacc_balances.get(&1).copied().unwrap(),
            bal_before + expected_rebate
        );
        // `DK-033`: fee 10_000 is split 50/50 (inflow 5_000, treasury +5_000), then
        // the 200 rebate leaves the treasury.
        assert_eq!(c.treasury, treasury_before + fee - expected_rebate - 5_000);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 5_000);
        assert_eq!(c.macro_state.epoch_treasury_spent, expected_rebate);
    }

    /// (3) The `I-04` invariant: the rebate stops when the 500 bps epoch quota runs out (no panic).
    #[test]
    fn vc005_invariant_i04_epoch_quota_500_bps_exhausted_no_panic() {
        let mut c = SnaccContract::new();
        // stake = 1_000_000 -> treasury = 200_000 -> max_epoch_drain (500 bps) = 10_000
        let _ = call_incubate(&mut c, 1_000_000u128, EXTRACTOR_ROLE, 0);
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();

        // Already used 9_900 of the 10_000 quota -> remaining quota = 100.
        c.macro_state.epoch_treasury_spent = 9_900;
        let bal_before = c.snacc_balances.get(&1).copied().unwrap();

        // Fee = 10_000 -> raw_rebate = 200, but the remaining quota is only 100 -> the rebate is capped to 100.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.epoch_treasury_spent, 10_000);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), bal_before + 100);

        // The next call when the quota is exhausted (spent == 10_000): rebate = 0, no panic!
        let bal_after_cap = c.snacc_balances.get(&1).copied().unwrap();
        let treasury_after_cap = c.treasury;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.epoch_treasury_spent, 10_000);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), bal_after_cap);
        // `DK-033`: fee 10_000 is split 50/50 — the treasury only +5_000 (rebate 0, quota exhausted).
        assert_eq!(c.treasury, treasury_after_cap + 5_000);
    }

    /// (4) `advance_epoch` resets the `epoch_treasury_spent` quota and snapshots `epoch_treasury_initial`.
    #[test]
    fn vc005_advance_epoch_reset_kuota() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000u128, EXTRACTOR_ROLE, 0);
        c.macro_state.epoch_treasury_spent = 10_000;

        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        assert_eq!(c.macro_state.current_epoch, 1);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.macro_state.epoch_treasury_initial, c.treasury);

        // After advance_epoch, the rebate is active again in the new epoch.
        let bal_before = c.snacc_balances.get(&1).copied().unwrap();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), bal_before + 200);
        assert_eq!(c.macro_state.epoch_treasury_spent, 200);
    }

    /// (3) The middle zone `600..=1400` is neutral: no burn and no rebate.
    #[test]
    fn vc005_middle_zone_neutral() {
        let mut c = SnaccContract::new();
        // stake = 10_000 -> supply = 10_000, treasury = 2_000, strain = 8_000
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);

        // The new Normal zone = [600, 610] (ECON-111: Fever >610, Hypo <600).
        // Order note: the thermostat evaluates the temperature from the fee BEFORE the K2/K3
        // legs are added; the legs are seen by the next evaluation (lazy).
        // fee 6_100 -> temp inside = 610 -> NORMAL (exactly at the upper bound): no burn/rebate.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(6_100));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(6_100));
        // volume after legs = 6_600 -> view temp = 660.
        assert_eq!(c.get_crucible_temp(), 660);
        assert_eq!(c.macro_state.total_snacc_burned, 0);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000);
        // `DK-033`: fee 6_100 is split 50/50 — treasury 2_000+3_050, inflow 3_050.
        assert_eq!(c.treasury, 5_050);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 3_050);

        // One step above the bound: fee 100 -> temp inside 670 > 610 -> FEVER.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(100));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(100));
        // burn 350 bps × 100 = 3 -> supply 9_997; volume 6_700+500 = 7_200
        // -> view temp = floor(1000×7_200/9_997) = 720.
        assert_eq!(c.get_crucible_temp(), 720);
        assert_eq!(c.macro_state.total_snacc_burned, 3);
        assert_eq!(c.macro_state.epoch_treasury_spent, 0);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000);
        // remainder 97: inflow 48, treasury +49.
        assert_eq!(c.treasury, 5_099);
        assert_eq!(c.macro_state.epoch_dividend_inflow, 3_098);
    }

    /// (2) `get_macro_state` consistent with `get_crucible_temp` & JSON/Borsh serialization.
    #[test]
    fn vc005_get_macro_state_and_get_crucible_temp_consistent() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(9_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(9_000));

        let temp = c.get_crucible_temp();
        let view = c.get_macro_state();
        // ECON-111: fee 9_000 + legs 500 -> volume 9_500; temp 950 > 610 (Fever)
        // -> burn 350 bps × 9_000 = 315 -> supply 9_685; temp = floor(1000×9_500/9_685) = 980.
        assert_eq!(temp, 980);
        assert_eq!(view.crucible_temp, temp);
        assert_eq!(view.total_snacc_supply, 9_685);
        assert_eq!(view.volume_epoch, 9_500);

        let json = serde_json::to_string(&view).unwrap();
        let back: state::MacroStateView = serde_json::from_str(&json).unwrap();
        assert_eq!(view, back);
    }

    /// (5) `ft_burn` burns the `snacc_balances[strain_id]` balance and keeps the `I-01` invariant.
    #[test]
    fn vc005_ft_burn_burns_strain_balance() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain=8_000, supply=10_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.ft_burn(1, U128(3_000));

        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 5_000);
        assert_eq!(c.macro_state.total_snacc_supply, 7_000);
        assert_eq!(c.macro_state.total_snacc_burned, 3_000);
        assert_eq!(c.get_strain(1).state_nonce, 1);
    }

    /// (5) `ft_burn` exceeding the strain's balance -> rejected `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn vc005_ft_burn_insufficient_balance_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain=8_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.ft_burn(1, U128(9_000));
    }

    // ------------------------------------------------------------------
    // VC-006 — F3: 2-phase zero-trust TEE + guardrail I-03
    // ------------------------------------------------------------------

    // A REAL cryptographic vector from the NEAR AI Cloud TEE (spike `S1.8`/`S2.6`,
    // `chat_id: c895b8f389fe45f3b369417a6de52536`) — verified by `env::ed25519_verify`.
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

    /// (a) verify the REAL signature (`env::ed25519_verify`) over the spike's signed_text.
    #[test]
    fn vc006_signature_real_vector_passes() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        assert!(SnaccContract::verify_tee_receipt(
            TEE_REQ_SHA,
            TEE_RES_SHA,
            TEE_SIG_HEX,
            TEE_PUBKEY_HEX
        ));
    }

    /// (a2) red-team `S1.9`: change 1 signature / hash byte -> verification fails.
    #[test]
    fn vc006_tamper_one_byte_rejected() {
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

    /// (b) commit binding: `req_sha256` differs from the commitment -> `ERR_COMMITMENT_MISMATCH`.
    #[test]
    #[should_panic(expected = "ERR_COMMITMENT_MISMATCH")]
    fn vc006_commitment_mismatch_rejected() {
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

    /// (c) `I-16`: unregistered key -> `ERR_SIGNER_NOT_REGISTERED`.
    #[test]
    #[should_panic(expected = "ERR_SIGNER_NOT_REGISTERED")]
    fn vc006_signer_unregistered_rejected() {
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

    /// (d) guardrail `I-03`: `efficiency_shift_bps` beyond ±1500 -> rejected.
    #[test]
    #[should_panic(expected = "ERR_GUARDRAIL_SHIFT")]
    fn vc006_guardrail_shift_over_1500_rejected() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = SnaccContract::parse_mutation_payload(&envelope(1, 1501, 0), 1);
    }

    /// (d2) guardrail `I-03`: `vitality_bonus > 500` -> rejected.
    #[test]
    #[should_panic(expected = "ERR_GUARDRAIL_VITALITY")]
    fn vc006_guardrail_vitality_over_500_rejected() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = SnaccContract::parse_mutation_payload(&envelope(1, 0, 501), 1);
    }

    /// (e) payload for another strain -> `ERR_PAYLOAD_STRAIN_MISMATCH`.
    #[test]
    #[should_panic(expected = "ERR_PAYLOAD_STRAIN_MISMATCH")]
    fn vc006_payload_for_other_strain_rejected() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = SnaccContract::parse_mutation_payload(&envelope(2, 450, 200), 1);
    }

    /// (f) parse a valid payload (spike vector: +450 bps, +200 vitality, trait).
    #[test]
    fn vc006_parse_payload_matches_spike_vector() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let p = SnaccContract::parse_mutation_payload(&envelope(1, 450, 200), 1);
        assert_eq!(p.efficiency_shift_bps, 450);
        assert_eq!(p.vitality_bonus, 200);
        assert_eq!(p.phenotype_trait.as_deref(), Some("Thermo-Aureate"));
    }

    /// (g) signer registration: the first owner bootstraps + non-owner rejected.
    #[test]
    #[should_panic(expected = "ERR_NOT_CONTRACT_OWNER")]
    fn vc006_register_signer_owner_only() {
        let mut c = SnaccContract::new();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.register_attested_signer(TEE_PUBKEY_HEX.to_string(), 1_700_000_000);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.register_attested_signer(
            "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            1_700_000_000,
        );
    }

    /// (h) a fake body with a real signature -> `ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH`
    /// (red-team `S1.9` replication: there is no way through without a genuinely signed body).
    #[test]
    #[should_panic(expected = "ERR_TEE_RESPONSE_HASH_OR_ED25519_MISMATCH")]
    fn vc006_apply_fake_body_rejected() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0)); // SEC-113: owner at deploy time
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
        // strain 1 (Extractor) & strain 2 (Fermenter), stake 10_000 -> balance 8_000 each.
        let _ = call_incubate(c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(c, 10_000u128, FERMENTER_ROLE, 0);
    }

    /// (a) asset merge: 97.5% $SNACC + 100% of 9 molecules + a 2.5% tax to the treasury.
    #[test]
    fn vc007_splice_merges_assets_and_tax() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c); // balance 8_000 each; treasury 4_000 (20% ×2)
        let _ = c.inventories.insert((2, "mt:spore-ignis".to_string()), 5);
        let _ = c.inventories.insert((2, "mt:enzyme-amalgam".to_string()), 3);
        let _ = c.inventories.insert((2, "mt:elixir-aether".to_string()), 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 2);
        // tax = 8000 * 250 / 10000 = 200; the remaining 7800 -> dominant.
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

    /// (c) `absorbed` -> `SplicedBurned`; re-splicing is rejected.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_ALREADY_SPLICED")]
    fn vc007_absorbed_burned_and_resplice_rejected() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 2);
        assert_eq!(c.get_strain(2).status, StrainStatus::SplicedBurned);
        let _ = c.splice_strains(1, 2);
    }

    /// (d) different owners -> `ERR_NOT_SAME_OWNER`.
    #[test]
    #[should_panic(expected = "ERR_NOT_SAME_OWNER")]
    fn vc007_different_owner_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(10_000), 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 2);
    }

    /// (e) same id -> rejected.
    #[test]
    #[should_panic(expected = "ERR_SAME_STRAIN")]
    fn vc007_same_id_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.splice_strains(1, 1);
    }

    /// `SEC-101`: `ft_burn` by a non-owner is rejected (the `free-pi-cli` finding: without a
    /// caller check, anyone could burn someone else's Strain balance).
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn sec101_ft_burn_non_owner_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.ft_burn(1, U128(1_000));
    }

    // ------------------------------------------------------------------
    // VC-008 — F6: harvest_starving_strain (Cell Lysis, ADR-11 / I-10)
    // ------------------------------------------------------------------

    /// (a) 90/10 split, permissionless, cash + inventory wiped, status Liquidated.
    #[test]
    fn vc008_harvest_split_90_10() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // balance 8_000; treasury 2_000
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 5);
        // 200 hours × BMR 100 = 20000 > 8500 -> effective_vitality = 0.
        set_context_at(
            "raider.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let (bounty, recovery) = c.harvest_starving_strain(1);
        assert_eq!(bounty.0, 800); // 10% of 8_000
        assert_eq!(recovery.0, 7_200); // 90%
        assert_eq!(c.account_balances.get(&"raider.testnet".parse::<AccountId>().unwrap()), Some(&800));
        assert_eq!(c.treasury, 9_200); // 2_000 + 7_200
        assert_eq!(c.snacc_balances.get(&1), Some(&0));
        assert_eq!(c.get_strain(1).status, StrainStatus::Liquidated);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&0));
    }

    /// (b) `I-10`: the target is still alive -> `ERR_STRAIN_NOT_INSOLVENT`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_INSOLVENT")]
    fn vc008_target_still_alive_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        let _ = c.harvest_starving_strain(1);
    }

    /// (c) double harvest -> `ERR_ALREADY_LIQUIDATED`.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn vc008_double_harvest_rejected() {
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

    /// (a) media = data URI base64, deterministic, sane payload.
    #[test]
    fn vc009_nft_token_media_deterministic() {
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
        // SVG ≤ 1,734 bytes (unit-tested in svg.rs) -> base64 payload ≤ ~2,340 characters.
        assert!(t1.metadata.media.len() < 2_400, "media {} byte", t1.metadata.media.len());
    }

    /// (b) an invalid / missing token id -> `ERR_STRAIN_NOT_FOUND`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn vc009_nft_token_missing_rejected() {
        let c = SnaccContract::new();
        let _ = c.nft_token("not-a-number".to_string());
    }

    /// (c) a liquidated strain can still be rendered (red) via `nft_token`.
    #[test]
    fn vc009_nft_token_after_liquidation() {
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

    /// (a) every state action emits `EVENT_JSON` with `standard:"snacc"`.
    /// Note: `testing_env!` resets the logs, so logs are checked per action.
    #[test]
    fn vc010_event_emitted_per_action() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 3); // -> strain_created
        let l1 = near_sdk::test_utils::get_logs().join("\n");
        assert!(l1.contains(r#""event":"strain_created""#), "strain_created missing");
        assert!(l1.contains(r#""standard":"snacc""#));

        set_context(ALKEMIS_ID, NearToken::from_yoctonear(5));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(5)); // -> extracted
        let l2 = near_sdk::test_utils::get_logs().join("\n");
        assert!(l2.contains(r#""event":"extracted""#), "extracted missing");

        set_context_at(
            "raider.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1); // -> strain_liquidated
        let l3 = near_sdk::test_utils::get_logs().join("\n");
        assert!(l3.contains(r#""event":"strain_liquidated""#), "strain_liquidated missing");
    }

    // ------------------------------------------------------------------
    // N141-101..106 — surface the NEP-141 standard ($SNACC)
    // ------------------------------------------------------------------

    const ALICE: &str = "alice.testnet";
    const BOB: &str = "bob.testnet";

    /// Register an account (the MVP 5-milliNEAR storage deposit).
    fn register(c: &mut SnaccContract, account: &str) {
        set_context(account, NearToken::from_millinear(constants::STORAGE_DEPOSIT_MILLINEAR));
        let _ = c.storage_deposit(None, None);
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

    /// (1) a successful transfer + a `NEP-297` event with standard `nep141`.
    #[test]
    fn n141_ft_transfer_success_and_event() {
        let (mut c, supply) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(ALICE.parse().unwrap(), U128(400), Some("lab fee".to_string()));
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 600);
        assert_eq!(c.account_balance(&ALICE.parse().unwrap()), 400);
        // The transfer does not change the supply (`I-01`).
        assert_eq!(c.macro_state.total_snacc_supply, supply);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""standard":"nep141""#), "standard nep141 missing");
        assert!(logs.contains(r#""event":"ft_transfer""#), "event ft_transfer missing");
        assert!(logs.contains(r#""old_owner_id":"alkemis.testnet""#));
        assert!(logs.contains(r#""new_owner_id":"alice.testnet""#));
    }

    /// (2) an unregistered receiver -> `ERR_ACCOUNT_NOT_REGISTERED`.
    #[test]
    #[should_panic(expected = "ERR_ACCOUNT_NOT_REGISTERED")]
    fn n141_transfer_to_unregistered_rejected() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer("raider.testnet".parse().unwrap(), U128(100), None);
    }

    /// (3) insufficient balance -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn n141_insufficient_balance_rejected() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 100);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(ALICE.parse().unwrap(), U128(400), None);
    }

    /// (4) without 1 yoctoNEAR -> `ERR_YOCTO_REQUIRED` (recorded decision: the spec does not name it).
    #[test]
    #[should_panic(expected = "ERR_YOCTO_REQUIRED")]
    fn n141_without_one_yocto_rejected() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.ft_transfer(ALICE.parse().unwrap(), U128(100), None);
    }

    /// (5) self-transfer is neutral: balance & total supply unchanged.
    #[test]
    fn n141_self_transfer_netral() {
        let (mut c, supply) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(ALKEMIS_ID.parse().unwrap(), U128(400), None);
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 1_000);
        assert_eq!(c.macro_state.total_snacc_supply, supply);
    }

    /// (6a) `ft_transfer_call`: the callback fails -> a full refund to the sender.
    #[test]
    fn n141_transfer_call_full_refund_on_failure() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 1_000);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.ft_transfer_call(ALICE.parse().unwrap(), U128(400), None, "x".to_string());
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 600);
        assert_eq!(c.account_balance(&ALICE.parse().unwrap()), 400);

        // Simulate the callback with a failed promise (`ft_on_transfer` does not respond).
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

    /// (6b) `ft_transfer_call`: `ft_on_transfer` succeeds using part of it -> the rest is refunded.
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

    /// (7) `ft_metadata` is correct (decimals 24 = recorded decision).
    #[test]
    fn n141_ft_metadata_correct() {
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

    /// (8) `ft_total_supply` consistent with `I-01` (grows on incubation, drops on burn).
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
    fn n141_storage_full_cycle() {
        let mut c = SnaccContract::new();
        register(&mut c, ALICE);
        let alice: AccountId = ALICE.parse().unwrap();
        let bal = c.storage_balance_of(alice.clone()).unwrap();
        assert_eq!(bal.total.0, NearToken::from_millinear(5).as_yoctonear());

        // Attach another deposit -> the total grows.
        set_context(ALICE, NearToken::from_millinear(3));
        let bal2 = c.storage_deposit(Some(alice.clone()), None);
        assert_eq!(bal2.total.0, NearToken::from_millinear(8).as_yoctonear());

        // Not holding $SNACC yet -> a full withdrawal is possible; the entry is removed (deregistration).
        set_context(ALICE, NearToken::from_yoctonear(0));
        let bal3 = c.storage_withdraw(None);
        assert_eq!(bal3.total.0, 0);
        assert_eq!(c.storage_balance_of(alice.clone()), None);

        // Holding $SNACC -> the 5 milliNEAR minimum stays locked.
        register(&mut c, ALICE);
        fund(&mut c, ALICE, 500);
        set_context(ALICE, NearToken::from_yoctonear(0));
        let bal4 = c.storage_withdraw(None);
        assert_eq!(bal4.total.0, NearToken::from_millinear(5).as_yoctonear());
        assert_eq!(bal4.available.0, 0);
        assert!(c.storage_balance_of(alice).is_some());
    }

    /// (10) `get_total_wealth` = account_balances + Σ of its strain balances.
    #[test]
    fn n141_get_total_wealth() {
        let (mut c, _) = n141_setup();
        fund(&mut c, ALKEMIS_ID, 100); // strain 1 belongs to ALKEMIS, balance 8_000
        assert_eq!(c.get_total_wealth(ALKEMIS_ID.parse().unwrap()).0, 8_100);
        assert_eq!(c.ft_balance_of(ALKEMIS_ID.parse().unwrap()).0, 100);
        assert_eq!(c.ft_balance_of(ALICE.parse().unwrap()).0, 0);
    }

    /// (11) a storage deposit below the minimum -> `ERR_INSUFFICIENT_DEPOSIT`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
    fn n141_storage_deposit_below_minimum_rejected() {
        let mut c = SnaccContract::new();
        set_context(ALICE, NearToken::from_millinear(4));
        let _ = c.storage_deposit(None, None);
    }

    // ------------------------------------------------------------------
    // DK-033 / DK-044 — Industrial Dividend Pool (ADR-28)
    // ------------------------------------------------------------------

    /// Dividend setup: strain 1 = Extractor (pays the catalyst fee), strain 2 =
    /// Synthesizer (dividend contributor/claimer). Supply 20_000, treasury 4_000.
    fn dk_setup() -> SnaccContract {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, SYNTHESIZER_ROLE, 0);
        c
    }

    /// Basic scheme: catalyst fee → inflow; burn aether → contribution; close the epoch →
    /// rate; an exact pro-rata claim.
    #[test]
    fn dk033_basic_dividend_pro_rata_scheme() {
        let mut c = dk_setup();
        // Epoch 0: fee 10_000 (temp = 1000, neutral zone) -> inflow 5_000.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        assert_eq!(c.macro_state.epoch_dividend_inflow, 5_000);

        // Burn 2 aether as a contribution (strain 2 = Synthesizer).
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 5);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(2));
        assert_eq!(c.macro_state.epoch_aether_burned_total, 2);
        assert_eq!(c.inventories.get(&(2, constants::ELIXIR_AETHER.to_string())), Some(&3));

        // Close the epoch: rate = 5_000 × 1e18 / 2 = 2_500 × 1e18; pool = 5_000.
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        assert_eq!(c.get_epoch_dividend_rate(0).0, 2_500_000_000_000_000_000_000); // 2_500 × 1e18
        assert_eq!(c.get_dividend_pool(), (U128(5_000), U128(0), 1));

        // Claim: payout = 2 × 2_500e18 / 1e18 = 5_000 (full pro-rata, the only contributor).
        let bal_before = c.snacc_balances.get(&2).copied().unwrap();
        let payout = c.claim_industrial_dividend(2, 0);
        assert_eq!(payout.0, 5_000);
        assert_eq!(c.snacc_balances.get(&2).copied().unwrap(), bal_before + 5_000);
        assert_eq!(c.get_dividend_pool().0 .0, 0);
        let (contrib, claimed) = c.get_dividend_contribution(0, 2);
        assert_eq!(contrib.0, 2);
        assert!(claimed);
        // `TC-DIV-10`: the nonce rises on payout (`ADR-29`).
        assert!(c.get_strain(2).state_nonce > 0);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"dividend_claimed""#));
    }

    /// `TC-DIV-03` — a double claim is rejected.
    #[test]
    #[should_panic(expected = "ERR_DIVIDEND_ALREADY_CLAIMED")]
    fn dk033_double_claim_rejected() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        let _ = c.claim_industrial_dividend(2, 0);
        let _ = c.claim_industrial_dividend(2, 0);
    }

    /// `TC-DIV-04` — claiming a running epoch -> `ERR_EPOCH_NOT_CLOSED`.
    #[test]
    #[should_panic(expected = "ERR_EPOCH_NOT_CLOSED")]
    fn dk033_claim_running_epoch_rejected() {
        let mut c = dk_setup();
        let _ = c.claim_industrial_dividend(1, 0);
    }

    /// The `MAX_DIVIDEND_CLAIM_EPOCHS = 8` lookback limit -> `ERR_DIVIDEND_CLAIM_EXPIRED`.
    #[test]
    #[should_panic(expected = "ERR_DIVIDEND_CLAIM_EXPIRED")]
    fn dk033_expired_claim_rejected() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        // Close 10 epochs (empty inflow in epochs 1..10).
        for _ in 0..10 {
            set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
            c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        }
        let _ = c.claim_industrial_dividend(2, 0);
    }

    /// `TC-DIV-05` — the payout is capped at the pool balance; never mints / goes negative.
    #[test]
    fn dk033_pool_cap_and_conservation() {
        let mut c = dk_setup(); // strain 1 Extractor, strain 2 Synthesizer
        // Fee 10_000 from strain 1 -> inflow 5_000.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        // A third strain = an extra Synthesizer.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(SYNTHESIZER_ROLE), U128(0), 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        // strains 2 & 3 burn 1 & 3 aether (total 4).
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 1);
        let _ = c.inventories.insert((3, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.synthesize_and_redeem_aether(3, U128(3));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        // ECON-111: the 2nd extract lands in Fever (temp 683 > 610) -> burn 350 ->
        // inflow = 5_000 + 4_825 = 9_825; rate = 9.825×1e18/4 = 2.45625e21.
        let pool = c.get_dividend_pool().0 .0;
        assert_eq!(pool, 9_825);
        let p2 = c.claim_industrial_dividend(2, 0).0;
        let p3 = c.claim_industrial_dividend(3, 0).0;
        assert_eq!(p2, 2_456); // 1 × rate / 1e18
        assert_eq!(p3, 7_368); // 3 × rate / 1e18
        // `TC-DIV-05`: Σ payout ≤ pool; the rounding remainder of 1 stays in the pool.
        assert!(p2 + p3 <= pool);
        assert_eq!(c.get_dividend_pool().0 .0, 1);
    }

    /// Empty pool (rate 0) -> `ERR_DIVIDEND_POOL_EMPTY`.
    #[test]
    #[should_panic(expected = "ERR_DIVIDEND_POOL_EMPTY")]
    fn dk033_empty_pool_rejected() {
        let mut c = dk_setup();
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch(); // no fee -> inflow 0 -> rate 0 -> payout 0
        let _ = c.claim_industrial_dividend(2, 0);
    }

    /// `TC-DIV-06` — without the `ROLE_SYNTHESIZER` bit -> `ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE`.
    #[test]
    #[should_panic(expected = "ERR_ROLE_LOCKED_NEEDS_SYNTHESIZER_GENE")]
    fn dk044_role_not_synthesizer_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = c.inventories.insert((1, constants::ELIXIR_AETHER.to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(1, U128(1));
    }

    /// `TC-DIV-07` — another account burns another strain's aether -> `ERR_NOT_STRAIN_OWNER`.
    #[test]
    #[should_panic(expected = "ERR_NOT_STRAIN_OWNER")]
    fn dk044_non_owner_rejected() {
        let mut c = dk_setup();
        let _ = c.inventories.insert((1, constants::ELIXIR_AETHER.to_string()), 3);
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(1, U128(1));
    }

    /// Insufficient aether stock / amount 0 (< `MIN_AETHER_BURN`) -> `ERR_INSUFFICIENT_AETHER`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_AETHER")]
    fn dk044_insufficient_aether_rejected() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
    }

    /// A starving strain (effective_vitality == 0) -> `ERR_STRAIN_STARVING`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_STARVING")]
    fn dk044_starving_strain_rejected() {
        let mut c = dk_setup();
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 3);
        set_context_at(
            ALKEMIS_ID,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        c.synthesize_and_redeem_aether(2, U128(1));
    }

    /// `TC-DIV-02` — anti first-mover: two strains, the claim order reversed, the same
    /// total payout (the rate is locked at `advance_epoch`).
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
        c.synthesize_and_redeem_aether(3, U128(1)); // strain 3 (small contribution) goes first
        c.synthesize_and_redeem_aether(2, U128(3));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        // ECON-111: inflow = 5_000 + 4_825 (Fever burn 350) = 9_825; rate = 2.45625e21;
        // a 3:1 contribution -> payout 7_368 : 2_456 (pro-rata, not first-mover).
        let p3 = c.claim_industrial_dividend(3, 0).0; // the claim order is reversed
        let p2 = c.claim_industrial_dividend(2, 0).0;
        assert_eq!(p3, 2_456);
        assert_eq!(p2, 7_368);
        assert_eq!(p2 + p3, 9_824);
    }

    // ------------------------------------------------------------------
    // EN-101..104 — Nutrient Burst (spec Enhancement §3)
    // ------------------------------------------------------------------

    /// `EN-101` — the `get_burst_state` getter (0 before any liquidation).
    #[test]
    fn en101_get_burst_state_starts_zero() {
        let c = dk_setup();
        assert_eq!(c.get_burst_state(), (U128(0), 0));
    }

    /// `EN-102` — `harvest_starving_strain` switches the burst on: `min(BASE + scaled, MAX)`,
    /// `until = epoch + 1`, the `nutrient_burst_released` event.
    #[test]
    fn en102_harvest_ignites_burst() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // balance 8_000, supply 10_000
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
        assert!(logs.contains(r#""event":"nutrient_burst_released""#), "burst event missing");
        assert!(logs.contains(r#""released_vita":7200"#));
        assert!(logs.contains(r#""until_epoch":1"#));
    }

    /// `TC-NB-02` — a second liquidation does not exceed `MAX_BURST_BPS`.
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

    /// `EN-103` — `feed_strain`: the burst discount lowers the effective fee; normal again after
    /// `until` (`TC-NB-03`); the `fed` event. Note: a small fee against a small supply is always
    /// in the Hypothermia zone (temp < 600) — the net debit = effective_fee − rebate.
    #[test]
    fn en103_feed_burst_discount_and_expiry() {
        let mut c = dk_setup(); // strain balance 8_000, treasury 4_000
        // Dormancy is disabled so the test focuses on the burst discount (dormancy is tested separately).
        c.macro_state.params.dormancy_threshold_bps = 0;
        // Switch the burst on manually (the same pattern as EN-102).
        c.macro_state.nutrient_burst_bps = 500;
        c.macro_state.nutrient_burst_until = 1;
        c.macro_state.epoch_treasury_initial = c.treasury;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.feed_strain(1, U128(1_000));
        // burst 500 -> 950; rescue (eff 8500) mult 8800 -> 950×8800/10000 = 836;
        // hypo: rebate 836×200/10000 = 16 -> net debit 820.
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        assert_eq!(bal0 - bal1, 836 - 16);
        assert_eq!(v.last_fed_sec, 3600); // metabolism recovered
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"fed""#), "event fed missing");
        assert!(logs.contains(r#""effective_fee":836"#));
        assert!(logs.contains(r#""burst_bps":500"#));
        assert!(logs.contains(r#""rescue_bps":8800"#));

        // `TC-NB-03`: the burst expires after `until` — the burst is gone (rescue stays).
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch(); // current_epoch = 1 == until -> the burst dies
        assert_eq!(c.get_burst_state(), (U128(0), 1));
        let bal2 = c.snacc_balances.get(&1).copied().unwrap();
        c.feed_strain(1, U128(1_000));
        let bal3 = c.snacc_balances.get(&1).copied().unwrap();
        // Tanpa burst: f1 = 1_000; rescue eff 9000 -> mult 9200 -> 920;
        // volume 836+920 = 1_756 -> temp 87 (hypo); rebate 920×200/10000 = 18.
        assert_eq!(bal2 - bal3, 920 - 18);
    }

    /// `EN-104` — the 5% Pantry discount cap per epoch (the `I-04` quota is shared with the rebate).
    #[test]
    fn en104_5_pantry_discount_cap_per_epoch() {
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0; // dormancy tested separately
        c.macro_state.nutrient_burst_bps = 2_000;
        c.macro_state.nutrient_burst_until = 1;
        // The I-04 quota = 500 bps of epoch_treasury_initial 10_000 = 500.
        c.macro_state.epoch_treasury_initial = 10_000;
        c.macro_state.epoch_burst_discount_granted = 490;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        // raw discount = 200; remaining quota 500−0−490 = 10 -> discount 10; f1 = 990;
        // rescue (eff 8500) mult 8800 -> f2 = 871; hypo: rebate 871×200/10000 = 17.
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        assert_eq!(bal0 - bal1, 871 - 17);
        assert_eq!(c.macro_state.epoch_burst_discount_granted, 500); // cap reached
        assert_eq!(c.macro_state.epoch_treasury_spent, 17);
    }

    /// `TC-NB-01` — a loop of 1,000 feeds while the burst is active: the player's balance drops net
    /// (`DK-030`: the discount must not make feeding profitable) + the `I-01` conservation.
    #[test]
    fn en104_tc_nb01_loop_1000_feeds_stay_net_loss() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000u128, EXTRACTOR_ROLE, 0); // balance 8e8, treasury 2e8
        c.macro_state.params.dormancy_threshold_bps = 0; // dormancy tested separately
        c.macro_state.nutrient_burst_bps = 2_000;
        c.macro_state.nutrient_burst_until = 1;
        c.macro_state.epoch_treasury_initial = c.treasury;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        for i in 0..1_000 {
            if i % 10 == 0 {
                // testing_env! recreates the mock blockchain -> the 16 KB log buffer is reset
                // (storage is kept); the 1,000-feed loop emits 2 events/feed.
                set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
            }
            c.feed_strain(1, U128(1_000));
        }
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        // `DK-030` (TC-NB-01): a clean net loss — every feed loses (the 200 bps rebate < the fee);
        // rescue lowers the fee while vitality rises, but never turns a profit.
        assert!(bal1 < bal0, "feeding must be a net loss");
        assert!(bal0 - bal1 > 500_000, "substantial net loss");
        // The rebate ≤ the fee burned/collected (`DK-030`): the subsidy quota ≤ the feed volume.
        assert!(c.macro_state.epoch_treasury_spent <= c.macro_state.epoch_feast_volume);
        // `DK-033` conservation (the running epoch's inflow is accounted separately); the supply is unchanged
        // because the zone is always below Fever (max volume ~8e5 << 1.4e9).
        assert_eq!(
            c.treasury
                + c.macro_state.industrial_dividend_pool
                + c.macro_state.epoch_dividend_inflow
                + c.macro_state.total_snacc_burned
                + bal1,
            1_000_000_000
        );
    }

    /// `EN-103` — feeding by a non-owner is rejected; a 0 feed is rejected.
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn en103_feed_non_owner_rejected() {
        let mut c = dk_setup();
        set_context("raider.testnet", NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(100));
    }

    /// `EN-103` — feeding beyond the strain's balance -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn en103_feed_insufficient_balance_rejected() {
        let mut c = dk_setup();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(9_000)); // balance 8_000
    }

    /// `F2` — aether can now be synthesized if the strain has had a successful TEE mutation
    /// (`phenotype_trait.is_some()`); without a mutation it stays `ERR_REQUIRES_TEE_ATTESTATION`.
    #[test]
    fn dk044_aether_synthesizes_after_tee_mutation() {
        let mut c = dk_setup();
        let mut v = c.strains.get(&2).unwrap().clone();
        v.phenotype_trait = Some("Thermo-Aureate".to_string());
        let _ = c.strains.insert(2, v);
        // The aether recipe (recorded decision): 2 amalgam + 1 synthase per batch.
        let _ = c.inventories.insert((2, "mt:enzyme-amalgam".to_string()), 2);
        let _ = c.inventories.insert((2, "mt:enzyme-synthase".to_string()), 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.synthesize_elixir(2, "mt:elixir-aether".to_string(), 3, U128(0));
        assert_eq!(c.inventories.get(&(2, "mt:elixir-aether".to_string())), Some(&1));
    }

    // ------------------------------------------------------------------
    // TASK 2 #1 — hostile_takeover_splice (Chapter 11 Hostile M&A, ADR-26/I-09/I-14)
    // ------------------------------------------------------------------

    const RAIDER: &str = "raider.testnet";

    /// Hostile setup: strain 1 = ALKEMIS Extractor (target), strain 2 = RAIDER Fermenter
    /// (stake 1_000_000 -> balance 800_000). The target is made distressed (eff 1500).
    fn hostile_setup() -> SnaccContract {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // target, bal 8_000
        set_context(RAIDER, NearToken::from_yoctonear(1_000_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(1_000_000), 0); // raider
        // Target distress: 70 hours × BMR 100 = 7_000 -> eff 1_500 (Tier 2: 1..=2000).
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        c
    }

    /// (a) a successful takeover: pay 115% of book value, split 95/5, absorb assets + role OR.
    #[test]
    fn hostile_success_split_95_5_and_absorb() {
        let mut c = hostile_setup();
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 5); // book +5
        // book = 8_000 + 5 = 8_005; payment = 8_005 × 115% = 9_205 (floor);
        // seller 95% = 8_744; treasury 5% = 461.
        let (payment, view) = c.hostile_takeover_splice(2, 1, 0);
        assert_eq!(payment.0, 9_205);
        assert_eq!(c.snacc_balances.get(&2).copied().unwrap(), 800_000 - 9_205 + 8_000);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 0);
        assert_eq!(c.account_balance(&ALKEMIS_ID.parse().unwrap()), 8_744);
        assert_eq!(c.treasury, 202_000 + 461); // 2_000+2_000 (incubations) + 200_000 (raider stake) + 461
        // Absorb: role OR (0b010 | 0b001), gen+1, max efficiency, inventory moves.
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

    /// (b) a healthy target (eff 8500 > 2000) -> `ERR_NOT_DISTRESSED` (I-09).
    #[test]
    #[should_panic(expected = "ERR_NOT_DISTRESSED")]
    fn hostile_not_distressed_rejected() {
        let mut c = hostile_setup();
        // The context returns to the initial ts: the target's eff is still 8500 (not yet 70 hours).
        set_context_at(RAIDER, NearToken::from_yoctonear(0), 3_600_000_000_000);
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    /// (c) `I-14`: nonce mismatch -> `ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED`.
    #[test]
    #[should_panic(expected = "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED")]
    fn hostile_nonce_mismatch_rejected() {
        let mut c = hostile_setup();
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.hostile_takeover_splice(2, 1, 99);
    }

    /// (d) `I-14`: a callback is running -> `ERR_PENDING_ASYNC_CALLBACK_LOCKED`.
    #[test]
    #[should_panic(expected = "ERR_PENDING_ASYNC_CALLBACK_LOCKED")]
    fn hostile_pending_callback_rejected() {
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

    /// (e) the caller does not own the raider -> `ERR_NOT_OWNED_BY_CALLEE` (V-01).
    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn hostile_raider_not_owned_by_caller() {
        let mut c = hostile_setup();
        set_context("intruder.testnet", NearToken::from_yoctonear(0));
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    /// (f) the raider's balance < 115% of book value -> `ERR_INSUFFICIENT_BALANCE`.
    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn hostile_raider_balance_insufficient() {
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

    /// (g) the target is already Liquidated -> `ERR_STRAIN_ALREADY_SPLICED`.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_ALREADY_SPLICED")]
    fn hostile_target_liquidated_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(1_000_000));
        let _ = c.incubate_strain(role(FERMENTER_ROLE), U128(1_000_000), 0);
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1); // the target becomes Liquidated
        set_context(RAIDER, NearToken::from_yoctonear(0));
        let _ = c.hostile_takeover_splice(2, 1, 0);
    }

    // ------------------------------------------------------------------
    // TASK 2 #2 — EN-105..112 (Great Feast, zones, leaderboard, dormancy, rescue, diet, meter)
    // ------------------------------------------------------------------

    /// `EN-105` — the Goldilocks band: UNDERFED / GOLDILOCKS / OVERFED measured.
    #[test]
    fn en105_feast_band_goldilocks() {
        // UNDERFED: no feeds at all.
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"feast_result""#));
        assert!(logs.contains(r#""verdict":"UNDERFED""#));

        // GOLDILOCKS: target = 5% × 20_000 = 1_000; band ±20% = [800, 1_200];
        // one feed of 1_000 -> effective 880 (rescue 8800) inside the band.
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""verdict":"GOLDILOCKS""#));
        assert!(logs.contains(r#""volume":880"#));
        assert_eq!(c.macro_state.epoch_feast_volume, 0); // reset per epoch

        // OVERFED: 3 feeds -> 880+920+960 = 2_760 > 1_200.
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        c.feed_strain(1, U128(1_000));
        c.feed_strain(1, U128(1_000));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""verdict":"OVERFED""#));
    }

    /// `EN-106` — the `zone_changed {from_zone, to_zone, crucible_temp}` event when the zone changes.
    #[test]
    fn en106_zone_changed_event() {
        let mut c = dk_setup(); // supply 20_000
        // Extract fee 10_000 -> temp inside = floor(1000×10_000/20_000) = 500.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"zone_changed""#));
        assert!(logs.contains(r#""from_zone":"NORMAL""#));
        assert!(logs.contains(r#""to_zone":"HYPO""#));
        // temp inside = floor(1000×10_000/20_000) = 500 (<600 -> HYPO; the K2/K3 legs
        // are added AFTER the temperature evaluation, seen by the next evaluation).
        assert!(logs.contains(r#""crucible_temp":500"#));
        // Extract again -> volume 21_000 -> temp 1025 > 610 -> HYPO->FEVER.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(10_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""from_zone":"HYPO""#));
        assert!(logs.contains(r#""to_zone":"FEVER""#));
        assert!(logs.contains(r#""crucible_temp":1025"#));
        assert_eq!(c.macro_state.last_zone, state::MacroZone::Fever);
    }

    /// `EN-107` — top-3 feeders per epoch, capped at 3 entries, sorted descending.
    #[test]
    fn en107_top_feeder_max_3() {
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
        c.feed_strain(4, U128(1_000)); // dave +880 (does not make the top-3)
        let top = c.get_top_feeder(0);
        assert_eq!(top.len(), 3, "capped at 3 entries");
        assert_eq!(top[0].account_id.to_string(), ALKEMIS_ID);
        assert_eq!(top[0].amount, 880 + 920);
        assert_eq!(top[1].account_id.to_string(), RAIDER);
        assert_eq!(top[2].account_id.to_string(), "carol.testnet");
        assert_eq!(
            c.feast_contributions.get(&(0, ALKEMIS_ID.parse::<AccountId>().unwrap())).copied(),
            Some(880 + 920)
        );
    }

    /// `EN-108` — dormancy: upkeep drops to 50 bps when volume < threshold; the transition is measured.
    #[test]
    fn en108_dormancy_upkeep_50_bps() {
        assert_eq!(state::constants::UPKEEP_MIN_BPS, 50);
        let mut c = dk_setup(); // supply 20_000; the default threshold 100 bps = 200
        assert!(c.get_appetite_meter().dormant); // volume 0 < 200
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(1_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        // f1 = 1_000; rescue 8800 -> 880; dormancy ×50/10000 -> 4.
        assert!(logs.contains(r#""effective_fee":4"#));
        assert!(logs.contains(r#""dormant":true"#));
        assert!(c.get_appetite_meter().dormant); // volume 4 still < 200
        // Transition: threshold 0 -> not dormant; the fee returns to full (rescue only).
        c.macro_state.params.dormancy_threshold_bps = 0;
        assert!(!c.get_appetite_meter().dormant);
        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        c.feed_strain(1, U128(1_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        // rescue eff 9000 -> 920 (no dormancy).
        assert!(logs.contains(r#""effective_fee":920"#));
        assert!(logs.contains(r#""dormant":false"#));
        assert!(c.snacc_balances.get(&1).copied().unwrap() < bal0);
    }

    /// `EN-109` — rescue feeding: the bounds 0 (20% fee) and 100% (full fee); linear in between.
    #[test]
    fn en109_rescue_bounds_0_and_100_percent() {
        let mut c = dk_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        let _ = c.snacc_balances.insert(1, 20_000); // enough balance for the full fee
        // Lower bound: eff = 1 -> multiplier = 2000 (RESCUE_MIN_FEE_BPS).
        let mut v = c.strains.get(&1).unwrap().clone();
        v.vitality = 1;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""effective_fee":2000"#)); // 10_000 × 2000/10000
        assert!(logs.contains(r#""rescue_bps":2000"#));

        // Upper bound: eff = 10000 -> multiplier 10000 (full fee).
        let mut v = c.strains.get(&1).unwrap().clone();
        v.vitality = constants::VITALITY_CAP; // 10000
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(10_000));
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""effective_fee":10000"#));
        assert!(logs.contains(r#""rescue_bps":10000"#));
    }

    /// `EN-110` + `EN-111` — per-caste per-zone diet matrix: 8 combinations
    /// (masks with the Fermenter bit: 0b010/0b011/0b110/0b111 × Fever/Hypo).
    #[test]
    fn en110_en111_diet_matrix_8_combinations() {
        // Fever (volume 30_000 -> temp 1500): bit Extractor -> 2000.
        // Hypo (volume 0 -> temp 0): bit Synthesizer -> 2000.
        let cases: [(u8, u128, u16); 8] = [
            (0b010, 30_000, 0),     // Fermenter @Fever: not an Extractor -> 0
            (0b010, 0, 0),          // Fermenter @Hypo: not a Synthesizer -> 0
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
            c.macro_state.last_update_ms = 3_600_000_000; // decay 0 (initial last_update 0)
            let effective = 1_000u128 * u128::from(10_000 - *expected) / 10_000;
            set_context(ALKEMIS_ID, NearToken::from_yoctonear(effective));
            let _ = c.transmute_spores_to_enzyme(1, 2, U128(1_000));
            let logs = near_sdk::test_utils::get_logs().join("\n");
            assert!(
                logs.contains(&format!(r#""diet_bps":{}"#, expected)),
                "case {}: diet_bps {} not reached",
                i,
                expected
            );
        }
    }

    /// `EN-110` — attached deposit != the fee after the discount -> rejected.
    #[test]
    #[should_panic(expected = "ERR_STAKE_DEPOSIT_MISMATCH")]
    fn en110_diet_fee_nominal_rejected() {
        let mut c = SnaccContract::new();
        c.macro_state.params.dormancy_threshold_bps = 0;
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011; // has the Extractor bit -> the Fever discount
        let _ = c.strains.insert(1, v);
        grant(&mut c, "mt:spore-ignis", 2);
        c.macro_state.volume_epoch = 30_000; // Fever
        c.macro_state.last_update_ms = 3_600_000_000;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000)); // should be 800
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(1_000));
    }

    /// `EN-112` — `get_appetite_meter`: the zone, burst countdown, dormancy status, feast.
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
    // TASK 3 — SEC-107..109
    // ------------------------------------------------------------------

    // A FRESH ed25519 vector (generated offline & self-verified) for the successful apply flow
    // + the `used_tee_chat_ids` anti-replay (SEC-107). Model: `z-ai/glm-5.3-flash`.
    const SEC_BODY: &str = r#"{"id":"sec-replay-test","choices":[{"message":{"content":"{\"strain_id\":1,\"efficiency_shift_bps\":100,\"vitality_bonus\":100,\"phenotype_trait\":\"SEC-Replay-Probe\"}"}}]}"#;
    const SEC_REQ_SHA: &str =
        "01c45b8519d21a933ffdf96737428001c6fa720afeadc1c1a6beeb66ba6055d4";
    const SEC_PUBKEY_HEX: &str =
        "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
    const SEC_SIG_HEX: &str = "b22529732798f14ce0e0645d357407ebb71ac6278238e357f0b85edba9ea6584295c52aa1d8dbba9b9fd8d681cb7c772dee0bd6d8f84e45b2208eb1a56f4f405";

    /// `SEC-107` — a fully successful TEE flow, then replaying the same `chat_id` is rejected
    /// (`used_tee_chat_ids`, further negative test).
    #[test]
    #[should_panic(expected = "ERR_TEE_REPLAY")]
    fn sec107_replaying_same_chat_id_rejected() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0)); // SEC-113: owner at deploy time
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
        // Successful mutation: +100 bps, +100 vitality, trait recorded.
        assert_eq!(v.efficiency_bps, 10_100);
        assert_eq!(v.vitality, 8_600);
        assert_eq!(v.phenotype_trait.as_deref(), Some("SEC-Replay-Probe"));
        // REPLAY: re-commit (the old commitment was already used) then the SAME body —
        // the `chat_id` "sec-replay-test" is already in `used_tee_chat_ids` -> rejected.
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

    /// `SEC-108` — `state_nonce`/`pending_callbacks` consistent with `ADR-29` on new mutations:
    /// feed & hostile bump the nonce; ft_transfer (the account ledger) does not touch strain
    /// nonces; hostile is blocked while a callback is running (tested in hostile_pending_callback).
    #[test]
    fn sec108_state_nonce_consistent_adr29() {
        let mut c = hostile_setup();
        c.macro_state.params.dormancy_threshold_bps = 0;
        assert_eq!(c.get_strain(1).state_nonce, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.feed_strain(1, U128(1_000));
        assert_eq!(c.get_strain(1).state_nonce, 1); // the feed bumps the nonce
        // ft_transfer between accounts does not change any StrainVessel nonce.
        fund(&mut c, ALKEMIS_ID, 100);
        register(&mut c, RAIDER);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(RAIDER.parse().unwrap(), U128(50), None);
        assert_eq!(c.get_strain(1).state_nonce, 1);
        assert_eq!(c.get_strain(2).state_nonce, 0);
        // hostile: both nonces rise (`record_mutation` on both sides).
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.hostile_takeover_splice(2, 1, 1); // target nonce = 1 (feed)
        assert!(c.get_strain(2).state_nonce > 0);
        assert!(c.get_strain(1).state_nonce > 0);
    }

    /// `SEC-109` — the `DK-030` anti-farming: a loop of 1,000 feeds + a dividend claim — the final
    /// balance still nets a loss; the total rebate ≤ the total fee collected.
    #[test]
    fn sec109_dk030_feed_loop_and_dividend_nets_loss() {
        let mut c = SnaccContract::new();
        // Strain 1 Extractor (pays the fee) + strain 2 Synthesizer (claims the dividend).
        let _ = call_incubate(&mut c, 1_000_000_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1_000_000_000));
        let _ = c.incubate_strain(role(SYNTHESIZER_ROLE), U128(1_000_000_000), 0);
        c.macro_state.params.dormancy_threshold_bps = 0;
        c.macro_state.epoch_treasury_initial = c.treasury;

        let bal0 = c.snacc_balances.get(&1).copied().unwrap();
        for i in 0..1_000 {
            if i % 10 == 0 {
                set_context(ALKEMIS_ID, NearToken::from_yoctonear(0)); // reset the mock log buffer
            }
            c.feed_strain(1, U128(1_000));
        }
        let bal1 = c.snacc_balances.get(&1).copied().unwrap();
        assert!(bal1 < bal0, "DK-030: the feed loop must net a loss");
        assert!(c.macro_state.epoch_treasury_spent <= c.macro_state.epoch_feast_volume);

        // Claim the dividend from the pool (burn aether + close the epoch + a full pro-rata claim).
        let _ = c.inventories.insert((2, constants::ELIXIR_AETHER.to_string()), 1);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.synthesize_and_redeem_aether(2, U128(1));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        let payout = c.claim_industrial_dividend(2, 0).0;
        assert!(payout > 0);
        // Even with the dividend fully claimed, the total is still a net loss (`DK-030`).
        let bal2 = c.snacc_balances.get(&1).copied().unwrap();
        assert!(bal2 + payout < bal0, "rebate+dividend must not exceed the fee");
    }

    // ------------------------------------------------------------------
    // Pipeline #3 — EN-120 (organel merger), EN-121 (colony), EN-122 (divide),
    // EN-123 (adopt), EN-125 (risk heat), NPC ADR-30
    // ------------------------------------------------------------------

    /// `EN-120` — the merger absorbs organelles (max per slot) + resets `last_allocate_epoch`.
    #[test]
    fn en120_splice_organelles_max_and_reset_allocation() {
        let mut c = SnaccContract::new();
        vc007_pair(&mut c); // strain 1 (Extractor) & 2 (Fermenter), same owner
        // Organelles: dominant mito=2, absorbed mito=3 → 3; nucleus dom=0, abs=1 → 1.
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
        assert_eq!(v.last_allocate_epoch, u64::MAX); // ATP reallocation reopens
    }

    /// `EN-120` — the hostile takeover also inherits organelles + resets the allocation.
    #[test]
    fn en120_hostile_organelles_max_and_reset_allocation() {
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

    /// `EN-121` — `get_colony`: the account's living cells; dead cells do not join the colony.
    #[test]
    fn en121_get_colony_filters_living_cells() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, SYNTHESIZER_ROLE, 0);
        let colony = c.get_colony(ALKEMIS_ID.parse().unwrap());
        assert_eq!(colony.len(), 3);
        assert!(colony.iter().all(|v| v.owner_id == ALKEMIS_ID));
        // Liquidate one cell -> it leaves the colony.
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        assert_eq!(c.get_colony(ALKEMIS_ID.parse().unwrap()).len(), 2);
        assert_eq!(c.colony_active_count(&ALKEMIS_ID.parse().unwrap()), 2);
        // Another account's colony is empty.
        assert_eq!(c.get_colony(RAIDER.parse().unwrap()).len(), 0);
    }

    /// `EN-122` — cytokinesis: Σ parent+child balance = parent balance; vitality & spores halved;
    /// organelles ÷2; the child's gen = parent+1; the `strain_divided` event.
    #[test]
    fn en122_divide_cytokinesis_conservation() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8_000, vitality 8500
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 7);
        let mut p = c.strains.get(&1).unwrap().clone();
        p.organel = state::OrganelleSlots { mitokondria: 3, nukleus: 1, lisosom: 0 };
        let _ = c.strains.insert(1, p);
        // `EN-124`: divide needs a demographics budget (merges+lyses >= divides) —
        // incubate one sacrificial cell then liquidate it (lyses = 1) as the budget.
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 2
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let (parent, child) = c.divide_strain(1);
        // Σ balance = 8_000 (`I-01`); vitality & spores halved.
        assert_eq!(parent.vitality, 4250);
        assert_eq!(child.vitality, 4250);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 4_000);
        // The child = strain 3 (strain 2 = the lysis victim for the EN-124 demographics budget).
        assert_eq!(c.snacc_balances.get(&3).copied().unwrap(), 4_000);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&4));
        assert_eq!(c.inventories.get(&(3, "mt:spore-ignis".to_string())), Some(&3));
        // Organelles ÷2: the child floors 3/2=1 mito, the parent keeps 2; the child's nucleus 0, the parent 1.
        assert_eq!(child.organel.mitokondria, 1);
        assert_eq!(parent.organel.mitokondria, 2);
        assert_eq!(child.organel.nukleus, 0);
        assert_eq!(parent.organel.nukleus, 1);
        // The genome is inherited; the child's gen = parent+1; the colony grows.
        assert_eq!(child.generation, 1);
        assert_eq!(child.role_mask, state::constants::ROLE_EXTRACTOR);
        assert_eq!(child.efficiency_bps, 10000);
        assert_eq!(child.owner_id.to_string(), ALKEMIS_ID);
        assert_eq!(child.vault_subaccount.to_string(), "s-3-0.snacc-v001.testnet");
        assert_eq!(child.strain_id, 3);
        assert_eq!(c.get_colony(ALKEMIS_ID.parse().unwrap()).len(), 2); // parent + child
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"strain_divided""#));
        assert!(logs.contains(r#""child_balance":4000"#));
        // The `D8` log comes only from divide (the F6 harvest does not write MnaLog — see the report).
        assert_eq!(c.mna_logs.len(), 1);
    }

    /// `EN-122` — the full-colony fence (`MAX_COLONY_SIZE = 5`).
    #[test]
    #[should_panic(expected = "ERR_COLONY_FULL")]
    fn en122_full_colony_rejected() {
        let mut c = SnaccContract::new();
        for _ in 0..5 {
            let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        }
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.divide_strain(1); // the colony is already 5
    }

    /// `EN-122` — the generation fence (`MAX_GENERATION = 8`).
    #[test]
    #[should_panic(expected = "ERR_MAX_GENERATION")]
    fn en122_max_generation_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let mut p = c.strains.get(&1).unwrap().clone();
        p.generation = 8;
        let _ = c.strains.insert(1, p);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.divide_strain(1); // the child's gen 9 > 8
    }

    /// `EN-123` — a successful adopt: pay book value WITHOUT the premium to the old owner,
    /// ownership moves, saved from lysis, the `strain_adopted` event.
    #[test]
    fn en123_adopt_success_ownership_moves() {
        let mut c = hostile_setup(); // target distress eff 1500; the raider belongs to RAIDER (bal 800_000)
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 2); // book = 8_002
        let old_owner_bal = c.account_balance(&ALKEMIS_ID.parse().unwrap());
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let v = c.adopt(1, 2, 0);
        // Ownership moves to the raider's owner; the target survives (status stays alive).
        assert_eq!(v.owner_id.to_string(), RAIDER);
        assert_eq!(c.get_strain(1).owner_id.to_string(), RAIDER);
        // The stored status stays alive (Active); the view downgrades to Distressed
        // because the effective vitality is still Tier 2 (audit W10).
        assert_eq!(c.strains.get(&1).unwrap().status, StrainStatus::Active);
        assert_eq!(c.get_strain(1).status, StrainStatus::Distressed);
        // Payment: 100% of book value to the old owner (no premium, no cut).
        assert_eq!(
            c.account_balance(&ALKEMIS_ID.parse().unwrap()),
            old_owner_bal + 8_002
        );
        assert_eq!(c.snacc_balances.get(&2).copied().unwrap(), 800_000 - 8_002);
        // The colony index updates on both sides.
        assert_eq!(c.colony_active_count(&ALKEMIS_ID.parse().unwrap()), 0);
        assert_eq!(c.colony_active_count(&RAIDER.parse().unwrap()), 2);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"strain_adopted""#));
        assert!(logs.contains(r#""book_value":8002"#));
        assert_eq!(c.mna_logs.len(), 1); // kind Adopt
    }

    /// `EN-123` — a healthy target (eff > 2000) cannot be adopted.
    #[test]
    #[should_panic(expected = "ERR_NOT_DISTRESSED")]
    fn en123_adopt_not_distressed_rejected() {
        let mut c = hostile_setup();
        set_context_at(RAIDER, NearToken::from_yoctonear(0), 3_600_000_000_000);
        let _ = c.adopt(1, 2, 0);
    }

    /// `EN-123` — the adopter's colony is full -> `ERR_COLONY_FULL`.
    #[test]
    #[should_panic(expected = "ERR_COLONY_FULL")]
    fn en123_adopt_full_colony_rejected() {
        let mut c = hostile_setup();
        for _ in 0..4 {
            set_context(RAIDER, NearToken::from_yoctonear(10_000));
            let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(10_000), 0);
        }
        // RAIDER now has 5 active cells -> the 6th adopt is rejected.
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.adopt(1, 2, 0);
    }

    /// `EN-123` — the target is already Liquidated -> `ERR_ALREADY_LIQUIDATED`.
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn en123_adopt_target_liquidated_rejected() {
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

    /// `EN-125` — `get_risk_heat`: the Fever-proximity base + burst/dividend bonuses; capped at 10000.
    #[test]
    fn en125_risk_heat_components_and_cap() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 1_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        // Fresh: temp 0, no burst, pool 0 → heat 0.
        assert_eq!(c.get_risk_heat(), 0);
        // Component 1: the temperature rises → heat = temp × 10000 / fever_threshold(610, ECON-111).
        c.macro_state.volume_epoch = 300_000_000_000u128; // temp = 1000×3e11/1e12 = 300
        c.macro_state.last_update_ms = 3_600_000;
        let expected = 300 * 10_000 / 610; // 4918
        assert_eq!(c.get_risk_heat(), expected);
        // Component 2: an active burst +1000.
        c.macro_state.nutrient_burst_bps = 2_000;
        c.macro_state.nutrient_burst_until = 1;
        assert_eq!(c.get_risk_heat(), expected + 1_000);
        // Component 3: the dividend pool ≥ 1% of supply +1000.
        c.macro_state.industrial_dividend_pool = c.macro_state.total_snacc_supply / 100;
        assert_eq!(c.get_risk_heat(), expected + 2_000);
        // Cap 10000: an extreme temp.
        c.macro_state.volume_epoch = u128::MAX / 4;
        assert_eq!(c.get_risk_heat(), 10_000);
        // Surfacing in the appetite meter.
        c.macro_state.volume_epoch = 0;
        c.macro_state.nutrient_burst_bps = 0;
        c.macro_state.industrial_dividend_pool = 0;
        let m = c.get_appetite_meter();
        assert_eq!(m.risk_heat, 0);
    }

    /// `ADR-30` — 3 NPCs seeded with the `D4` profile.
    #[test]
    fn npc_three_seeded() {
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
    fn npc_unknown_rejected() {
        let c = SnaccContract::new();
        let _ = c.get_npc("npc-hantu".to_string());
    }

    /// `ADR-30`/`I-15` — quote: unit price = floor(P_base × S_target / S_after), rises as stock shrinks.
    #[test]
    fn npc_quote_formula_scarcity() {
        let c = SnaccContract::new();
        // extractor 500/500: buy 100 → S_after 400 → unit = floor(100×500/400) = 125.
        let q = c.npc_quote_spores("npc-extractor".to_string(), 100);
        assert_eq!(q.unit_price_vita, 125);
        assert_eq!(q.total_cost_snacc, 12_500);
        assert_eq!(q.npc_spores_stock_after, 400);
        assert!(q.reasoning_tooltip.contains("Scarcity"));
        // A small buy: the price stays P_base while S_after stays near the target (floor rounding).
        let q2 = c.npc_quote_spores("npc-vulture-ma".to_string(), 1);
        assert_eq!(q2.unit_price_vita, 100); // floor(100×300/299) = 100
        // The price rises the more depleted: buying 299 from vulture → S_after 1 → unit = 30_000.
        let q3 = c.npc_quote_spores("npc-vulture-ma".to_string(), 299);
        assert_eq!(q3.unit_price_vita, 30_000);
    }

    /// `I-15` — exact conservation: Δreserve == −Δbalance == total_cost; Δstock == Δinventory.
    #[test]
    fn npc_buy_conservation_i15() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // buy 40 → S_after 460 → unit = floor(50_000/460) = 108 → total 4_320.
        let q = c.npc_buy_spores(1, "npc-extractor".to_string(), 40);
        assert_eq!(q.unit_price_vita, 108);
        assert_eq!(q.total_cost_snacc, 4_320);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000 - 4_320);
        let npc = c.get_npc("npc-extractor".to_string());
        assert_eq!(npc.snacc_reserve, 4_320); // == the balance decrease (I-15)
        assert_eq!(npc.spores_stock, 460); // == the inventory increase (I-15)
        assert_eq!(npc.trades_executed, 1);
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&40));
        assert_eq!(c.get_strain(1).state_nonce, 1); // `ADR-29`
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"npc_spores_bought""#));
    }

    #[test]
    #[should_panic(expected = "ERR_NPC_OUT_OF_STOCK")]
    fn npc_insufficient_stock_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 501);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn npc_buy_non_owner_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(0));
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 1);
    }

    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn npc_buy_insufficient_balance_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8_000
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // buy 100 → total 12_500 > 8_000.
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 100);
    }
    // ------------------------------------------------------------------
    // Pipeline #4 — EN-114..119 (ATP/mitochondria) + ECON-111 calibration
    // ------------------------------------------------------------------

    /// `EN-115`/§13a — the `calc_atp_minted` formula: base + the mitochondria bonus +10%/level.
    #[test]
    fn atp115_formula_and_mitochondria_bonus() {
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // fee 10 × 10^15 -> base 10; eff 10000 -> 10.
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 0), 10);
        // Mitochondria +10%/level: L1 → 11, L2 → 12, L5 → 15.
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 1), 11);
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 2), 12);
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 5), 15);
        // efficiency 5000 → half; a fee below 1 unit → 0.
        assert_eq!(SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 5_000, 0), 5);
        assert_eq!(SnaccContract::calc_atp_minted(999, 10_000, 5), 0);
    }

    /// `EN-114` — incubation grants `INITIAL_ATP = 50` (birth energy).
    #[test]
    fn atp114_incubation_init_50() {
        let mut c = SnaccContract::new();
        let v = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(v.atp_balance, constants::INITIAL_ATP); // 50
        assert_eq!(v.last_allocate_epoch, u64::MAX); // never allocated yet
    }

    /// `EN-06`/§13a — feeding mints ATP (a large fee ≥ 10^15) + shows in the `fed` event.
    #[test]
    fn atp114_feed_mints_atp_and_event() {
        let mut c = SnaccContract::new();
        // stake 1e16 → the strain's balance 8e15 = 8 ATP-yocto units (enough for 2 feeds).
        let _ = call_incubate(&mut c, 10_000_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        c.macro_state.params.dormancy_threshold_bps = 0;
        // full vitality → the rescue multiplier 10000 → effective = amount.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.vitality = constants::VITALITY_CAP;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.feed_strain(1, U128(2 * constants::ATP_YOCTO_UNIT));
        // mitochondria 0 → atp_minted = 2.
        assert_eq!(c.get_strain(1).atp_balance, 52);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""atp_minted":2"#), "atp_minted missing from the fed event");
        // Mitochondria bonus: set L2 → feed 5 units → minted = floor(5×1.2) = 6.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.organel.mitokondria = 2;
        let _ = c.strains.insert(1, v);
        c.feed_strain(1, U128(5 * constants::ATP_YOCTO_UNIT));
        assert_eq!(c.get_strain(1).atp_balance, 52 + 6);
    }

    /// `EN-116`/§13a — extract consumes 1 ATP; exhausted → `ERR_NOT_ENOUGH_ATP`.
    #[test]
    #[should_panic(expected = "ERR_NOT_ENOUGH_ATP")]
    fn atp116_extract_consumes_and_exhausts() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
        assert_eq!(c.get_strain(1).atp_balance, 49);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""purpose":"extract","cost":1"#));
        // Drain ATP → the next extract is rejected.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.atp_balance = 0;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_ENOUGH_ATP")]
    fn atp116_transmute_without_atp_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 0);
        grant(&mut c, "mt:spore-ignis", 2);
        let mut v = c.strains.get(&1).unwrap().clone();
        v.atp_balance = 1; // transmute needs 2
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
    }

    /// `EN-116`/§13a — splice 5 & hostile 10.
    #[test]
    fn atp116_costs_splice_and_hostile() {
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

    /// `EN-118`/§13a — `allocate_atp`: a cost of 1 per level increase, the `atp_allocated` event.
    #[test]
    fn atp118_allocate_full_cycle() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // atp 50
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.allocate_atp(1, 2, 1, 1);
        assert_eq!(v.organel.mitokondria, 2);
        assert_eq!(v.organel.nukleus, 1);
        assert_eq!(v.organel.lisosom, 1);
        assert_eq!(v.atp_balance, 46); // cost 2+1+1 = 4
        assert_eq!(v.last_allocate_epoch, 0);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"atp_allocated""#));
        assert!(logs.contains(r#""cost":4"#));
    }

    #[test]
    #[should_panic(expected = "ERR_ALREADY_ALLOCATED")]
    fn atp118_allocate_twice_in_epoch_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 1, 0, 0);
        let _ = c.allocate_atp(1, 2, 0, 0);
    }

    #[test]
    #[should_panic(expected = "ERR_ORGANEL_LEVEL_MAX")]
    fn atp118_level_six_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 6, 0, 0);
    }

    /// `EN-118` — a new epoch reopens allocation; the cost is only the level INCREASE.
    #[test]
    fn atp118_new_epoch_costs_increase_only() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 2, 1, 1); // cost 4 → 46
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let v = c.allocate_atp(1, 3, 1, 1); // the mito increase 1 → cost 1 → 45
        assert_eq!(v.atp_balance, 45);
        assert_eq!(v.last_allocate_epoch, 1);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn atp118_allocate_non_owner_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 1, 0, 0);
    }

    /// `EN-119` — nucleus & lysosome: NO invented economic effect; stored & displayed.
    #[test]
    fn en119_nucleus_lysosome_displayed_without_economic_effect() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.allocate_atp(1, 1, 4, 5);
        let v = c.get_strain(1);
        assert_eq!(v.organel.nukleus, 4);
        assert_eq!(v.organel.lisosom, 5);
        // The only economic effect = mitochondria (the §13a ATP formula).
        assert_eq!(
            SnaccContract::calc_atp_minted(10 * constants::ATP_YOCTO_UNIT, 10_000, 1),
            11
        );
    }

    /// `ECON-111` — the K2/K3/K4 legs enter `volume_epoch`; the mandatory exclusions do not.
    #[test]
    fn econ111_volume_multi_legs_and_exclusions() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8_000
        // K3: extract 1 spore → +1×5×100 = 500.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(0));
        assert_eq!(c.macro_state.volume_epoch, 500);
        // K2+K3: transmute 2 spores → burn 2, mint 2 → +2_000 → total 2_500.
        // (the extractor is given the Fermenter bit via a manual merger for the test)
        grant(&mut c, "mt:spore-ignis", 2); // enough stock for 1 batch (2 spores)
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011;
        let _ = c.strains.insert(1, v);
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert_eq!(c.macro_state.volume_epoch, 2_500);
        // EXCLUSION: stake capital does NOT add volume.
        set_context(RAIDER, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(0), 0);
        assert_eq!(c.macro_state.volume_epoch, 2_500);
        // EXCLUSION: ft_transfer does NOT add volume.
        fund(&mut c, ALKEMIS_ID, 100);
        register(&mut c, ALKEMIS_ID);
        register(&mut c, RAIDER);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.ft_transfer(RAIDER.parse().unwrap(), U128(50), None);
        assert_eq!(c.macro_state.volume_epoch, 2_500);
        // K4: liquidating strain 2 (insolvent) → volume += bounty+recovery = the full balance.
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
    // Pipeline #5 — LC-16 (nft_transfer), LC-20 (conjugation), LC-11 (burn discount),
    // EN-124 (demographics invariant), SEC-110 (adopt nonce), get_colony_atp
    // ------------------------------------------------------------------

    /// LC-16 — a successful transplantation: the whole cell moves (balance, spores, organelles, ATP),
    /// the colony index updates on both sides, the standard `nep171` event.
    #[test]
    fn lc16_nft_transfer_success() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // bal 8.000, atp 50
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 3);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.nft_transfer(
            ALICE.parse().unwrap(),
            "1".to_string(),
            None,
            Some("transplant".to_string()),
        );
        let v = c.get_strain(1);
        assert_eq!(v.owner_id.to_string(), ALICE);
        assert_eq!(c.snacc_balances.get(&1).copied().unwrap(), 8_000); // the stomach comes along
        assert_eq!(c.inventories.get(&(1, "mt:spore-ignis".to_string())), Some(&3));
        assert_eq!(v.atp_balance, 50); // ATP follows the vessel
        // The two-sided index.
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
    fn lc16_nft_transfer_without_yocto_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c.nft_transfer(ALICE.parse().unwrap(), "1".to_string(), None, None);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn lc16_nft_transfer_not_owner_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(RAIDER, NearToken::from_yoctonear(1));
        c.nft_transfer(ALICE.parse().unwrap(), "1".to_string(), None, None);
    }

    /// LC-16 — dead cells cannot be transferred (recorded decision).
    #[test]
    #[should_panic(expected = "ERR_ALREADY_LIQUIDATED")]
    fn lc16_nft_transfer_dead_cell_rejected() {
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
    fn lc16_nft_transfer_missing_token_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        c.nft_transfer(ALICE.parse().unwrap(), "99".to_string(), None, None);
    }

    /// LC-16 — `nft_transfer_call`: the receiver accepts (resolve `false`).
    #[test]
    fn lc16_transfer_call_accepted() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.nft_transfer_call(
            ALICE.parse().unwrap(),
            "1".to_string(),
            None,
            None,
            "sell".to_string(),
        );
        // Simulate the callback: the receiver's contract ACCEPTS (false).
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
        assert!(c.nft_resolve_transfer(
            ALKEMIS_ID.parse().unwrap(),
            ALICE.parse().unwrap(),
            "1".to_string(),
            None,
        ));
        assert_eq!(c.get_strain(1).owner_id.to_string(), ALICE);
    }

    /// LC-16 — `nft_transfer_call`: a failed promise → the token returns to the old owner.
    #[test]
    fn lc16_transfer_call_failure_returns() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(1));
        let _ = c.nft_transfer_call(
            ALICE.parse().unwrap(),
            "1".to_string(),
            None,
            None,
            "sell".to_string(),
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
        assert!(!c.nft_resolve_transfer(
            ALKEMIS_ID.parse().unwrap(),
            ALICE.parse().unwrap(),
            "1".to_string(),
            None,
        ));
        // Returns to the old owner + the index recovers.
        assert_eq!(c.get_strain(1).owner_id.to_string(), ALKEMIS_ID);
        assert_eq!(c.get_colony(ALKEMIS_ID.parse().unwrap()).len(), 1);
        assert_eq!(c.get_colony(ALICE.parse().unwrap()).len(), 0);
    }

    /// LC-20 — conjugation: the trait is copied (the source keeps it), the target's ATP cost 2.
    #[test]
    fn lc20_conjugate_success() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 2, same owner
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        // The source (1) had a successful TEE mutation (trait present); the target (2) has not.
        let mut f = c.strains.get(&1).unwrap().clone();
        f.phenotype_trait = Some("Thermo-Aureate".to_string());
        let _ = c.strains.insert(1, f);
        let v = c.conjugate_traits(
            1,
            2,
            "Thermo-Aureate".to_string(),
        );
        assert_eq!(v.phenotype_trait.as_deref(), Some("Thermo-Aureate"));
        assert_eq!(v.atp_balance, 48); // 50 - 2 (paid by the target)
        // The source keeps its trait (copy, not move).
        assert_eq!(
            c.get_strain(1).phenotype_trait.as_deref(),
            Some("Thermo-Aureate")
        );
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"trait_conjugated""#));
    }

    #[test]
    #[should_panic(expected = "ERR_TRAIT_MISMATCH")]
    fn lc20_conjugate_trait_mismatch_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 2, same owner
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.conjugate_traits(1, 2, "Trait-Nonsense".to_string());
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_SAME_OWNER")]
    fn lc20_conjugate_different_owner_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // strain 1 = ALKEMIS
        set_context(RAIDER, NearToken::from_yoctonear(10_000));
        let _ = c.incubate_strain(role(EXTRACTOR_ROLE), U128(10_000), 0); // strain 2 = RAIDER
        set_context(RAIDER, NearToken::from_yoctonear(0));
        // the caller = RAIDER owns the source (2), but the target (1) belongs to ALKEMIS.
        let mut f = c.strains.get(&2).unwrap().clone();
        f.phenotype_trait = Some("Thermo-Aureate".to_string());
        let _ = c.strains.insert(2, f);
        let _ = c.conjugate_traits(2, 1, "Thermo-Aureate".to_string());
    }

    /// LC-11 — the `mt:elixir-homeostasis` holder gets a 50% Fever-burn discount.
    #[test]
    fn lc11_homeostasis_holder_burn_discount() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // supply 10_000
        let _ = c.inventories.insert((1, "mt:elixir-homeostasis".to_string()), 1);
        let supply_before = c.macro_state.total_snacc_supply;
        // fee 20_000 → temp 2000 > 610 (Fever); the holder's burn = 20_000×350×50/(100×10_000) = 350.
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(20_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(20_000));
        let burn = supply_before - c.macro_state.total_snacc_supply;
        assert_eq!(burn, 350); // half of 700
        // The elixir is not consumed (passive effect).
        assert_eq!(
            c.inventories.get(&(1, "mt:elixir-homeostasis".to_string())),
            Some(&1)
        );
    }

    #[test]
    fn lc11_non_holder_full_burn() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let supply_before = c.macro_state.total_snacc_supply;
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(20_000));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(20_000));
        let burn = supply_before - c.macro_state.total_snacc_supply;
        assert_eq!(burn, 700); // the full 350 bps
    }

    /// EN-124 — the demographics gate: a divide without budget (merges+lyses < divides) is rejected.
    #[test]
    #[should_panic(expected = "ERR_BIRTHS_EXCEEDED")]
    fn en124_gate_without_budget_rejected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(c.macro_state.total_births, 2);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.divide_strain(1); // divides 1 > merges 0 + lyses 0
    }

    /// EN-124 — a lysis opens the divide budget; counters stay consistent.
    #[test]
    fn en124_gate_and_counters() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert_eq!(c.macro_state.total_births, 2);
        // Liquidating one cell → lyses = 1 → the divide budget opens.
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
        assert_eq!(c.macro_state.total_births, 3); // 2 incubations + 1 divide
        // Invariant: divides (1) <= mergers (0) + lyses (1).
        assert!(c.macro_state.total_divides
            <= c.macro_state.total_mergers + c.macro_state.total_lyses);
    }

    /// EN-124 — hostile bumps `total_mergers`.
    #[test]
    fn en124_hostile_bumps_mergers() {
        let mut c = hostile_setup();
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.hostile_takeover_splice(2, 1, 0);
        assert_eq!(c.macro_state.total_mergers, 1);
        assert_eq!(c.macro_state.total_births, 2); // 2 initial incubations
        assert_eq!(c.macro_state.total_lyses, 0);
    }

    /// SEC-110 — `adopt` with a stale nonce -> `ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED`.
    #[test]
    #[should_panic(expected = "ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED")]
    fn sec110_adopt_stale_nonce_rejected() {
        let mut c = hostile_setup();
        set_context_at(
            RAIDER,
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 70 * 3_600_000_000_000,
        );
        let _ = c.adopt(1, 2, 99); // nonce target = 0
    }

    /// EN-121 — `get_colony_atp`: Σ ATP of the active colony.
    #[test]
    fn en121_get_colony_atp() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // atp 50
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0); // atp 50
        assert_eq!(c.get_colony_atp(ALKEMIS_ID.parse().unwrap()).0, 100);
        // A large feed adds ATP to strain 1: +4 → 104. (is fee 4e15 ≤ the 8_000 balance? no —
        // use a large stake so it is enough.)
        let mut c2 = SnaccContract::new();
        let _ = call_incubate(&mut c2, 1_000_000_000_000_000_000u128, EXTRACTOR_ROLE, 0);
        c2.macro_state.params.dormancy_threshold_bps = 0;
        let mut v = c2.strains.get(&1).unwrap().clone();
        v.vitality = constants::VITALITY_CAP;
        let _ = c2.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        c2.feed_strain(1, U128(4 * constants::ATP_YOCTO_UNIT));
        assert_eq!(c2.get_colony_atp(ALKEMIS_ID.parse().unwrap()).0, 54);
        // Dead cells are not counted.
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

    /// J3.8 — the balance of one molecule; unknown token/strain = 0 (no panic).
    #[test]
    fn pipeline6_get_inventory_present_and_zero() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 3); // 3 genesis spores
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        assert_eq!(
            c.get_inventory(1, "mt:spore-ignis".to_string()).0,
            3
        );
        // Unknown token → 0, no panic.
        assert_eq!(c.get_inventory(1, "mt:bogus".to_string()).0, 0);
        // Unknown strain → 0, no panic.
        assert_eq!(c.get_inventory(99, "mt:spore-ignis".to_string()).0, 0);
        // After transmute: spores decrease, an enzyme appears.
        // `grant` OVERWRITES (does not add): 3 genesis → 5 → burn 2 → 3.
        grant(&mut c, "mt:spore-ignis", 5);
        // Give the Fermenter bit (extractor + fermenter) so the transmute is valid.
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011;
        let _ = c.strains.insert(1, v);
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert_eq!(c.get_inventory(1, "mt:spore-ignis".to_string()).0, 3);
        assert_eq!(c.get_inventory(1, "mt:enzyme-amalgam".to_string()).0, 2);
    }

    /// J3.9 — `V-04` solvent across a chain of actions (incubation, fee, feed,
    /// transmute, liquidation, NPC) + `drift == 0` at every point.
    #[test]
    fn pipeline6_solvency_solvent_after_actions() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, FERMENTER_ROLE, 2); // supply 10_000
        // Two role bits (extract for the fee + ferment for the transmute).
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011;
        let _ = c.strains.insert(1, v);
        let s0 = c.get_global_solvency();
        assert!(s0.solvent, "drift {}", s0.drift);
        // The catalyst fee (external NEAR-yocto inflow).
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(100));
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(100));
        let s1 = c.get_global_solvency();
        assert!(s1.solvent, "drift after extract: {}", s1.drift);
        assert_eq!(s1.total_catalyst_fees, 100);
        // Feed (debits the strain balance + the hypo rebate).
        set_context(ALKEMIS_ID, NearToken::from_yoctonear(0));
        let _ = c.feed_strain(1, U128(1_000));
        assert!(c.get_global_solvency().solvent);
        // Transmute (K2/K3 volume; burns spores, mints enzymes — untouched $SNACC).
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(0));
        assert!(c.get_global_solvency().solvent);
        // NPC buy (strain cash → NPC reserve).
        let _ = c.npc_buy_spores(1, "npc-vulture-ma".to_string(), 1);
        let s4 = c.get_global_solvency();
        assert!(s4.solvent, "drift after NPC: {}", s4.drift);
        assert_eq!(s4.sum_npc_reserve, c.get_npc("npc-vulture-ma".to_string()).snacc_reserve);
        // Liquidating another strain (bounty → the account; recovery → the treasury).
        set_context_at(
            "hunter.testnet",
            NearToken::from_yoctonear(0),
            3_600_000_000_000 + 200 * 3_600_000_000_000,
        );
        let _ = c.harvest_starving_strain(1);
        let s5 = c.get_global_solvency();
        assert!(s5.solvent, "drift after liquidation: {}", s5.drift);
        // Consistency with `get_macro_state`.
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

    /// J3.9 — injected bookkeeping corruption is detected: `solvent = false`, the drift is reported.
    #[test]
    fn pipeline6_solvency_drift_detected() {
        let mut c = SnaccContract::new();
        let _ = call_incubate(&mut c, 10_000u128, EXTRACTOR_ROLE, 0);
        assert!(c.get_global_solvency().solvent);
        // Simulate a bookkeeping bug: the aggregate is manipulated directly.
        c.macro_state.sum_strain_snacc += 5;
        let s = c.get_global_solvency();
        assert!(!s.solvent);
        assert_eq!(s.drift, -5);
    }
}

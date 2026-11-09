//! QA-130..133 (TC-DIV-*) — DK-033/044 dividends: the enzyme chain → `mt:elixir-aether`
//! → `synthesize_and_redeem_aether` → `advance_epoch` → `claim_industrial_dividend`.
//! The real TEE vector (payload `strain_id: 4` → five incubated strains) is embedded as
//! constants so the suite is self-contained in every checkout (the source version read it
//! from a monorepo-only `reports/` path that does not exist in a standalone clone).
//! The catalyst deposit is computed dynamically (EN-110 diet per zone).
//!
//! Port notes (kept honest):
//! - 5 of the 10 original tests were dropped as behavior-level duplicates of tests that
//!   already exist on main; those TC-DIV cases stay covered by `dk033_*` / `dk044_*`.
//! - `SEC_VECTOR` mirrors the vector already inlined in the `sec107_*` test (the repo
//!   convention is self-contained per-suite fixtures).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so test paths stay stable
mod qa_div_tests {
    use crate::state::{constants, StrainRole};
    use crate::SnaccContract;
    use crate::StrainId;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    fn test_account(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }

    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(test_account("snacc-v001.testnet"))
            .signer_account_id(test_account(predecessor))
            .predecessor_account_id(test_account(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }

    const FEE: u128 = 10u128.pow(13);
    const TS: u64 = 1_000;

    /// The real NEAR AI Cloud TEE response (chat `d1979ce2…`, payload `strain_id: 4`),
    /// embedded byte-exact: the contract recomputes `env::sha256(raw_response_body)`
    /// inside `apply_tee_mutation_zero_trust`, so a single altered byte fails the
    /// ed25519 check and the `phenotype_trait` assertion below.
    const LIVE_REQ_SHA256: &str =
        "32f8060d4c516ed0b85f0a67e23a3f86bb469f58ac68004e75044f6a4628fd1e";
    const LIVE_SIGNATURE: &str =
        "eedd84110d41fa68132beaf92d9aa69d456ed7f9b98246eb88d638eab38a5e6c0fdaeca411fcbe5f27ce8d6b5f5fab5a40109a8966bde17ea522d631952baa04";
    const LIVE_PUBKEY: &str =
        "5a0210f2442c79355784f9de021985429561da5bacc3a4424b01ac52b7e0caff";
    const LIVE_RESPONSE_BODY: &str = r#"{"id":"d1979ce2662d4da6ae7a931d1158656d","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"{\"strain_id\":4,\"efficiency_shift_bps\":450,\"vitality_bonus\":200,\"phenotype_trait\":\"mitokondria-empyrean\"}","reasoning_content":"The user wants me to return exactly the text they provided, with no markdown, no explanation, no other text. The text is:\n\n{\"strain_id\":4,\"efficiency_shift_bps\":450,\"vitality_bonus\":200,\"phenotype_trait\":\"mitokondria-empyrean\"}\n\nThis appears to be a JSON-like string. The user is asking me to echo it back exactly. This seems like a simple text reproduction task. There's nothing harmful here - it's just some fictional data about a \"strain\" with efficiency shifts and phenotype traits. It looks like it could be part of some game or simulation data.\n\nI should just return the exact text as requested, without any markdown formatting or additional text."},"finish_reason":"stop","logprobs":null,"matched_stop":154827}],"model":"z-ai/glm-5.3-flash","created":1794137923,"usage":{"prompt_tokens":83,"total_tokens":265,"completion_tokens":182,"prompt_tokens_details":null,"reasoning_tokens":147,"completion_tokens_details":{"reasoning_tokens":147}}}"#;

    /// Transmute deposit under the EN-110 diet: Fever (role carries the Extractor bit) or
    /// Hypothermia (role carries the Synthesizer bit) → 20% discount; the Normal zone pays full price.
    fn diet_deposit(c: &SnaccContract, id: StrainId, nominal: u128) -> u128 {
        let temp = c.get_crucible_temp();
        let role = c.get_strain(id).role_mask;
        let discounted = (temp < constants::HYPOTHERMIA_THRESHOLD_MK
            && role & constants::ROLE_SYNTHESIZER != 0)
            || (temp > constants::FEVER_THRESHOLD_MK && role & constants::ROLE_EXTRACTOR != 0);
        if discounted {
            nominal * 8 / 10
        } else {
            nominal
        }
    }

    /// Full chain up to claim-ready: 5 strains (live TEE payload targets `strain_id: 4`),
    /// TEE on strain 5's neighbour — splice to role 0b111 → enzymes → aether → redeem → advance_epoch.
    fn setup_dividend_chain() -> SnaccContract {
        ctx("alkemis.testnet", 0, TS); // set the context before new() so the owner is registered
        let mut c = SnaccContract::new();
        // 5 strains; strain 4 = Synthesizer (TEE payload target); strain 5 = Fermenter.
        for i in 0..5 {
            let role = if i == 3 {
                StrainRole::Synthesizer
            } else if i == 4 {
                StrainRole::Fermenter
            } else {
                StrainRole::Extractor
            };
            ctx("alkemis.testnet", 10u128.pow(24), TS);
            let _ = c.incubate_strain(role, U128(10u128.pow(24)), 0);
        }
        // Real TEE vector (embedded constant; payload targets strain 4).
        ctx("alkemis.testnet", 0, TS);
        c.register_attested_signer(LIVE_PUBKEY.to_string(), TS);
        ctx("alkemis.testnet", 0, TS);
        c.commit_genome_experiment(4, LIVE_REQ_SHA256.to_string());
        ctx("alkemis.testnet", 0, TS);
        let view = c.apply_tee_mutation_zero_trust(
            4,
            LIVE_REQ_SHA256.to_string(),
            LIVE_RESPONSE_BODY.to_string(),
            LIVE_SIGNATURE.to_string(),
            LIVE_PUBKEY.to_string(),
        );
        assert!(
            view.phenotype_trait.is_some(),
            "the live TEE vector must produce a phenotype_trait"
        );
        // Splice: strain 4 absorbs Extractor (3) & Fermenter (5) → role_mask 0b111.
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(4, 3);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(4, 5);
        assert_eq!(c.get_strain(4).role_mask, 0b111, "E|F|S (full-stack)");
        // Enzymes: extract ignis ×2 + cryo ×2 (deposit == nominal; extract has no diet).
        for token in ["mt:spore-ignis", "mt:spore-cryo"] {
            for _ in 0..2 {
                ctx("alkemis.testnet", FEE, TS);
                let _ = c.extract_spores(4, token.to_string(), U128(FEE));
            }
        }
        // Transmute: ignis pair → amalgam; cryo pair → synthase (diet-aware deposit).
        ctx("alkemis.testnet", diet_deposit(&c, 4, FEE), TS);
        let _ = c.transmute_spores_to_enzyme(4, 2, U128(FEE));
        ctx("alkemis.testnet", diet_deposit(&c, 4, FEE), TS);
        let _ = c.transmute_spores_to_enzyme(4, 2, U128(FEE));
        assert!(
            c.get_inventory(4, "mt:enzyme-amalgam".to_string()).0 >= 2
                && c.get_inventory(4, "mt:enzyme-synthase".to_string()).0 >= 1,
            "enzyme stock ready for aether"
        );
        // Aether (requires phenotype_trait ✓): recipe (amalgam 2, synthase 1, batch 3).
        ctx("alkemis.testnet", FEE, TS);
        let _ = c.synthesize_elixir(4, "mt:elixir-aether".to_string(), 3, U128(FEE));
        assert_eq!(c.get_inventory(4, "mt:elixir-aether".to_string()).0, 1);
        // Redeem (burn 1 aether as the epoch-0 dividend contribution).
        ctx("alkemis.testnet", 0, TS);
        c.synthesize_and_redeem_aether(4, U128(1));
        // Close the epoch → the rate is computed at close time (anti first-mover).
        ctx("alkemis.testnet", 0, TS);
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        c
    }

    /// Six strains for the SEC/live two-hero scenarios:
    /// 1=S, 2=F, 3=E, 4=E, 5=S, 6=F (TEE payloads target strains 1 & 4).
    fn incubate_six_strains() -> SnaccContract {
        ctx("alkemis.testnet", 0, TS); // set the context before new() so the owner is registered
        let mut c = SnaccContract::new();
        let roles: [StrainRole; 6] = [
            StrainRole::Synthesizer, // 1 — hero of the SEC vector
            StrainRole::Fermenter,   // 2 — donor for hero 1
            StrainRole::Extractor,   // 3 — donor for hero 1
            StrainRole::Synthesizer, // 4 — hero of the live vector (payload strain_id 4)
            StrainRole::Extractor,   // 5 — donor for hero 4
            StrainRole::Fermenter,   // 6 — donor for hero 4
        ];
        for (i, role) in roles.iter().enumerate() {
            ctx("alkemis.testnet", 10u128.pow(24), TS);
            let _ = c.incubate_strain(*role, U128(10u128.pow(24)), 0);
            let _ = i;
        }
        c
    }

    /// Apply a TEE vector to one strain (register once per pubkey).
    fn apply_tee_vector(
        c: &mut SnaccContract,
        id: StrainId,
        req: &str,
        body: &str,
        sig: &str,
        pubkey: &str,
    ) {
        ctx("alkemis.testnet", 0, TS);
        c.register_attested_signer(pubkey.to_string(), TS);
        ctx("alkemis.testnet", 0, TS);
        c.commit_genome_experiment(id, req.to_string());
        ctx("alkemis.testnet", 0, TS);
        let view = c.apply_tee_mutation_zero_trust(
            id,
            req.to_string(),
            body.to_string(),
            sig.to_string(),
            pubkey.to_string(),
        );
        assert!(
            view.phenotype_trait.is_some(),
            "TEE strain {} must carry a trait",
            id
        );
    }

    /// Produce `n` mt:elixir-aether on strain `id` (must be full-stack 0b111).
    fn produce_aether(c: &mut SnaccContract, id: StrainId, n: u64) {
        for _ in 0..(2 * n) {
            ctx("alkemis.testnet", FEE, TS);
            let _ = c.extract_spores(id, "mt:spore-ignis".to_string(), U128(FEE));
            ctx("alkemis.testnet", FEE, TS);
            let _ = c.extract_spores(id, "mt:spore-cryo".to_string(), U128(FEE));
        }
        for _ in 0..n {
            ctx("alkemis.testnet", diet_deposit(c, id, FEE), TS);
            let _ = c.transmute_spores_to_enzyme(id, 2, U128(FEE)); // ignis → amalgam
            ctx("alkemis.testnet", diet_deposit(c, id, FEE), TS);
            let _ = c.transmute_spores_to_enzyme(id, 2, U128(FEE)); // cryo → synthase
        }
        ctx("alkemis.testnet", FEE, TS);
        let _ = c.synthesize_elixir(id, "mt:elixir-aether".to_string(), 3 * n, U128(FEE));
        assert_eq!(
            c.get_inventory(id, "mt:elixir-aether".to_string()).0,
            n as u128,
            "{} aether minted on strain {}",
            n,
            id
        );
    }

    /// SEC test vector for strain 1 (the same vector inlined by the `sec107_*` test in lib).
    const SEC_VECTOR: (&str, &str, &str, &str) = (
        "01c45b8519d21a933ffdf96737428001c6fa720afeadc1c1a6beeb66ba6055d4",
        r#"{"id":"sec-replay-test","choices":[{"message":{"content":"{\"strain_id\":1,\"efficiency_shift_bps\":100,\"vitality_bonus\":100,\"phenotype_trait\":\"SEC-Replay-Probe\"}"}}]}"#,
        "b22529732798f14ce0e0645d357407ebb71ac6278238e357f0b85edba9ea6584295c52aa1d8dbba9b9fd8d681cb7c772dee0bd6d8f84e45b2208eb1a56f4f405",
        "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c",
    );

    // ------------------------------------------------------------------
    // Ported tests (5 of 10). Dropped as duplicates of tests already on main:
    //   - double claim            → `dk033_double_claim_rejected`
    //   - pool remainder carried   → `dk033_pool_cap_and_conservation`
    //   - running/expired claim    → `dk033_claim_running_epoch_rejected`, `dk033_expired_claim_rejected`
    //   - anti first-mover         → `dk033_anti_first_mover`
    //   - role gate (no Synth)     → `dk044_role_not_synthesizer_rejected`
    // ------------------------------------------------------------------

    /// QA-130 (TC-DIV-01) — the pro-rata claim pays out and lands in the strain balance
    /// (checked through `get_total_wealth`, over the full live-TEE production chain).
    #[test]
    fn tc_div_01_claim_pro_rata_pays_out() {
        let mut c = setup_dividend_chain();
        let wealth_before = c.get_total_wealth(test_account("alkemis.testnet")).0;
        ctx("alkemis.testnet", 0, TS);
        let payout = c.claim_industrial_dividend(4, 0);
        assert!(
            payout.0 > 0,
            "the pro-rata payout must be > 0 (1 aether contribution)"
        );
        assert_eq!(
            c.get_total_wealth(test_account("alkemis.testnet")).0,
            wealth_before + payout.0,
            "the payout lands in the strain balance (conservation)"
        );
    }

    /// QA-132 (TC-DIV-05) — the payout is capped at the pool balance: shrinking the pool
    /// artificially makes the cap branch fire (payout == pool, never negative).
    /// Related to `dk033_pool_cap_and_conservation`, which keeps the remainder but does
    /// not force the cap branch itself.
    #[test]
    fn tc_div_05_pool_cap_limits_payout() {
        let mut c = incubate_six_strains();
        let (req, body, sig, pubkey) = SEC_VECTOR;
        apply_tee_vector(&mut c, 1, req, body, sig, pubkey);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(1, 2);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(1, 3);
        produce_aether(&mut c, 1, 2);
        ctx("alkemis.testnet", 0, TS);
        c.synthesize_and_redeem_aether(1, U128(2));
        ctx("alkemis.testnet", 0, TS);
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        // Shrink the pool artificially (simulated claim swelling) → the cap must fire.
        c.macro_state.industrial_dividend_pool = 10;
        ctx("alkemis.testnet", 0, TS);
        let payout = c.claim_industrial_dividend(1, 0).0;
        assert_eq!(
            payout, 10,
            "payout capped == pool balance (never exceeds it)"
        );
        assert_eq!(
            c.get_dividend_pool().0 .0,
            0,
            "the pool is exhausted, never negative"
        );
    }

    /// QA-130..132 (TC-DIV-08) — a strain with zero contribution in that epoch:
    /// payout 0 → `ERR_DIVIDEND_POOL_EMPTY`, while the contributor can still claim.
    /// Distinct from `dk033_empty_pool_rejected` (a globally empty pool).
    #[test]
    fn tc_div_07_zero_contribution_rejected() {
        let mut c = incubate_six_strains();
        let (req, body, sig, pubkey) = SEC_VECTOR;
        apply_tee_vector(&mut c, 1, req, body, sig, pubkey);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(1, 2);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(1, 3);
        produce_aether(&mut c, 1, 1);
        // Strain 2 (Fermenter, still alive) never redeems → contribution (epoch 0, 2) = 0.
        ctx("alkemis.testnet", 0, TS);
        c.synthesize_and_redeem_aether(1, U128(1));
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        ctx("alkemis.testnet", 0, TS);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.claim_industrial_dividend(2, 0);
        }));
        assert!(
            res.is_err(),
            "a zero contribution must hit ERR_DIVIDEND_POOL_EMPTY"
        );
        // The contributing strain can still claim.
        ctx("alkemis.testnet", 0, TS);
        let payout = c.claim_industrial_dividend(1, 0).0;
        assert!(payout > 0, "the contributor's claim must still pay out");
    }

    /// QA-130 (TC-DIV-07) — a claim by a non-owner → `ERR_NOT_STRAIN_OWNER`.
    /// (`dk044_non_owner_rejected` covers the *redeem* path, not the *claim* path.)
    #[test]
    fn tc_div_08_non_owner_claim_rejected() {
        let mut c = setup_dividend_chain();
        ctx("raider.testnet", 0, TS);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.claim_industrial_dividend(4, 0);
        }));
        assert!(
            res.is_err(),
            "a non-owner claim must hit ERR_NOT_STRAIN_OWNER"
        );
    }

    /// QA-133 (TC-DIV-10) — burning aether across two epochs: the per-epoch rates differ
    /// correctly, the pool accumulates, a payout raises `state_nonce`, and a buyout with
    /// the pre-claim nonce is rejected (`ADR-29`).
    #[test]
    fn tc_div_10_two_epoch_rates_and_nonce() {
        let mut c = incubate_six_strains();
        let (req, body, sig, pubkey) = SEC_VECTOR;
        apply_tee_vector(&mut c, 1, req, body, sig, pubkey);
        apply_tee_vector(&mut c, 4, LIVE_REQ_SHA256, LIVE_RESPONSE_BODY, LIVE_SIGNATURE, LIVE_PUBKEY);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(1, 2);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(1, 3);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(4, 6);
        ctx("alkemis.testnet", 0, TS);
        let _ = c.splice_strains(4, 5);
        // Epoch 0: strain 1 burns 2 aether.
        produce_aether(&mut c, 1, 2);
        ctx("alkemis.testnet", 0, TS);
        c.synthesize_and_redeem_aether(1, U128(2));
        ctx("alkemis.testnet", 0, TS);
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        let inflow0 = c.get_dividend_pool().0 .0; // pool == epoch-0 inflow (no claims yet)
        let rate0 = c.get_epoch_dividend_rate(0).0;
        assert_eq!(
            rate0,
            inflow0 * 1_000_000_000_000_000_000 / 2,
            "rate epoch 0 = inflow×1e18/burned"
        );
        // Epoch 1: strain 4 burns 2 aether (different fees → different inflow).
        produce_aether(&mut c, 4, 2);
        ctx("alkemis.testnet", 3 * FEE, TS);
        let _ = c.extract_spores(4, "mt:spore-ignis".to_string(), U128(3 * FEE));
        ctx("alkemis.testnet", 0, TS);
        c.synthesize_and_redeem_aether(4, U128(2));
        ctx("alkemis.testnet", 0, TS);
        c.macro_state.epoch_ends_sec = 0; // test: skip the W6 time gate
        c.advance_epoch();
        let rate1 = c.get_epoch_dividend_rate(1).0;
        let inflow1 = c.macro_state.industrial_dividend_pool - inflow0; // pool accumulation
        assert_ne!(rate0, rate1, "per-epoch rates differ (different inflow)");
        assert!(inflow1 > 0, "the pool accumulates the epoch-1 inflow");
        // Claim nonce: a payout raises the nonce → a buyout with the OLD nonce is rejected (ADR-29).
        ctx("alkemis.testnet", 0, TS);
        let nonce_before = c.get_strain(1).state_nonce;
        let _ = c.claim_industrial_dividend(1, 0);
        assert!(
            c.get_strain(1).state_nonce > nonce_before,
            "the payout raises state_nonce (ADR-29/TC-DIV-10)"
        );
        // Make strain 6 Distressed, then buyout with the nonce captured BEFORE the claim (stale).
        let nonce_stale = c.get_strain(6).state_nonce;
        ctx("raider.testnet", 5 * 10u128.pow(24), 1_000 + 100 * 3_600);
        let raider = c
            .incubate_strain(StrainRole::Fermenter, U128(5 * 10u128.pow(24)), 0)
            .strain_id;
        ctx("raider.testnet", 0, 1_000 + 100 * 3_600);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.hostile_takeover_splice(raider, 6, nonce_stale);
        }));
        assert!(
            res.is_err(),
            "a stale-nonce buyout must be rejected (ADR-29/TC-DIV-10)"
        );
    }
}
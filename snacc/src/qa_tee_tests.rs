//! QA-111..114 (TC-TEE-*) — the zero-trust 2-phase TEE flow: commitment, `I-03`
//! guardrail, `I-06` anti-replay, payload mismatch, signer registry `I-16`, plus the
//! SEC-113 / DK-070/073/074 hardening tests.
//! The real TEE vector (payload `strain_id:4`) is embedded as constants so the suite
//! is self-contained in every checkout (the source version read it from a monorepo-only
//! `reports/` path that does not exist in a standalone clone).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so test paths stay stable
mod qa_tee_tests {
    use crate::state::StrainRole;
    use crate::SnaccContract;
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

    /// The real NEAR AI Cloud TEE vector (chat `d1979ce2…`, payload targets strain 4),
    /// embedded byte-exact: the contract recomputes `env::sha256(raw_response_body)`
    /// inside `apply_tee_mutation_zero_trust`, so a single altered byte fails the check.
    const LIVE_REQ_SHA256: &str =
        "32f8060d4c516ed0b85f0a67e23a3f86bb469f58ac68004e75044f6a4628fd1e";
    const LIVE_SIGNATURE: &str =
        "eedd84110d41fa68132beaf92d9aa69d456ed7f9b98246eb88d638eab38a5e6c0fdaeca411fcbe5f27ce8d6b5f5fab5a40109a8966bde17ea522d631952baa04";
    const LIVE_PUBKEY: &str =
        "5a0210f2442c79355784f9de021985429561da5bacc3a4424b01ac52b7e0caff";
    const LIVE_CHAT_ID: &str = "d1979ce2662d4da6ae7a931d1158656d";
    const LIVE_RESPONSE_BODY: &str = r#"{"id":"d1979ce2662d4da6ae7a931d1158656d","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"{\"strain_id\":4,\"efficiency_shift_bps\":450,\"vitality_bonus\":200,\"phenotype_trait\":\"mitokondria-empyrean\"}","reasoning_content":"The user wants me to return exactly the text they provided, with no markdown, no explanation, no other text. The text is:\n\n{\"strain_id\":4,\"efficiency_shift_bps\":450,\"vitality_bonus\":200,\"phenotype_trait\":\"mitokondria-empyrean\"}\n\nThis appears to be a JSON-like string. The user is asking me to echo it back exactly. This seems like a simple text reproduction task. There's nothing harmful here - it's just some fictional data about a \"strain\" with efficiency shifts and phenotype traits. It looks like it could be part of some game or simulation data.\n\nI should just return the exact text as requested, without any markdown formatting or additional text."},"finish_reason":"stop","logprobs":null,"matched_stop":154827}],"model":"z-ai/glm-5.3-flash","created":1794137923,"usage":{"prompt_tokens":83,"total_tokens":265,"completion_tokens":182,"prompt_tokens_details":null,"reasoning_tokens":147,"completion_tokens_details":{"reasoning_tokens":147}}}"#;

    /// A live-vector bundle (four fields + the chat id used by the anti-replay check).
    struct LiveVector {
        req: String,
        body: String,
        sig: String,
        pubkey: String,
        chat_id: String,
    }

    fn live_vector() -> LiveVector {
        LiveVector {
            req: LIVE_REQ_SHA256.to_string(),
            body: LIVE_RESPONSE_BODY.to_string(),
            sig: LIVE_SIGNATURE.to_string(),
            pubkey: LIVE_PUBKEY.to_string(),
            chat_id: LIVE_CHAT_ID.to_string(),
        }
    }

    /// Setup: 5 strains (the vector payload targets `strain_id:4`), signer registered.
    fn setup_five_strains() -> (SnaccContract, LiveVector) {
        // SEC-113: the owner is set at deploy time → the context MUST precede new().
        ctx("alkemis.testnet", 0, 1_000);
        let mut c = SnaccContract::new();
        for _ in 0..5 {
            ctx("alkemis.testnet", 10u128.pow(24), 1_000);
            let _ = c.incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0);
        }
        let v = live_vector();
        ctx("alkemis.testnet", 0, 1_000);
        c.register_attested_signer(v.pubkey.clone(), 1_700_000_000);
        (c, v)
    }

    /// QA-111 (TC-TEE-01) — 2 phases: commit → apply succeeds (`genome_mutated`,
    /// efficiency +450, trait); a second commit while pending →
    /// `ERR_COMMITMENT_ALREADY_PENDING`; apply with req ≠ pending →
    /// `ERR_COMMITMENT_MISMATCH`.
    #[test]
    fn tc_tee_01_two_phase_and_double_commit() {
        let (mut c, v) = setup_five_strains();
        ctx("alkemis.testnet", 0, 1_000);
        c.commit_genome_experiment(4, v.req.clone());
        // A second commit while the first is still pending → rejected.
        ctx("alkemis.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.commit_genome_experiment(4, v.req.clone());
        }));
        assert!(
            res.is_err(),
            "a double commit while pending must hit ERR_COMMITMENT_ALREADY_PENDING"
        );
        // Apply with req ≠ pending → rejected.
        ctx("alkemis.testnet", 0, 1_000);
        let res2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.apply_tee_mutation_zero_trust(
                4,
                "11".repeat(32),
                v.body.clone(),
                v.sig.clone(),
                v.pubkey.clone(),
            );
        }));
        assert!(
            res2.is_err(),
            "req ≠ pending must hit ERR_COMMITMENT_MISMATCH"
        );
        // The correct apply → succeeds.
        ctx("alkemis.testnet", 0, 1_000);
        let view = c.apply_tee_mutation_zero_trust(
            4,
            v.req.clone(),
            v.body.clone(),
            v.sig.clone(),
            v.pubkey.clone(),
        );
        assert_eq!(view.efficiency_bps, 10_450, "shift +450 bps from the live vector");
        assert_eq!(
            view.phenotype_trait.as_deref(),
            Some("mitokondria-empyrean"),
            "trait from the live payload"
        );
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""event":"genome_mutated""#));
    }

    /// QA-112 (TC-TEE-02) — the `I-03` guardrail REJECTS (it does not clamp):
    /// `assert!(|shift| <= 1500)` / `assert!(vitality <= 500)` inside
    /// `parse_mutation_payload`; `saturating_add` is only the in-bounds overflow guard.
    /// The inclusive bounds 1500/500 are valid.
    #[test]
    fn tc_tee_02_guardrail_i03_rejects_not_clamps() {
        let body = |shift: i64, vit: u64| {
            format!(
                r#"{{"id":"g","choices":[{{"message":{{"content":"{}"}}}}]}}"#,
                format!(
                    r#"{{"strain_id":5,"efficiency_shift_bps":{},"vitality_bonus":{},"phenotype_trait":"T"}}"#,
                    shift, vit
                )
                .replace('"', "\\\"")
            )
        };
        ctx("alkemis.testnet", 0, 1_000);
        // +1501 → rejected.
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = SnaccContract::parse_mutation_payload(&body(1501, 0), 5);
        }));
        assert!(res.is_err(), "shift > 1500 must be rejected");
        // −1501 → rejected too.
        let res2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = SnaccContract::parse_mutation_payload(&body(-1501, 0), 5);
        }));
        assert!(res.is_err() && res2.is_err(), "shift < −1500 must be rejected");
        // vitality 501 → rejected.
        let res3 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = SnaccContract::parse_mutation_payload(&body(0, 501), 5);
        }));
        assert!(res3.is_err(), "vitality > 500 must be rejected");
        // Inclusive bounds: +1500 & vitality 500 → valid (not clamped — values applied as-is).
        let p = SnaccContract::parse_mutation_payload(&body(1500, 500), 5);
        assert_eq!(p.efficiency_shift_bps, 1500);
        assert_eq!(p.vitality_bonus, 500);
    }

    /// QA-113 (TC-TEE-03) — anti-replay `I-06`: the same `chat_id` used twice →
    /// `ERR_TEE_REPLAY` (live vector; commit again first because the commitment is one-shot).
    #[test]
    fn tc_tee_03_anti_replay_chat_id() {
        let (mut c, v) = setup_five_strains();
        ctx("alkemis.testnet", 0, 1_000);
        c.commit_genome_experiment(4, v.req.clone());
        ctx("alkemis.testnet", 0, 1_000);
        let _ = c.apply_tee_mutation_zero_trust(
            4,
            v.req.clone(),
            v.body.clone(),
            v.sig.clone(),
            v.pubkey.clone(),
        );
        // Replay: commit again (same req) then the SAME body/signature.
        ctx("alkemis.testnet", 0, 1_000);
        c.commit_genome_experiment(4, v.req.clone());
        ctx("alkemis.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.apply_tee_mutation_zero_trust(
                4,
                v.req.clone(),
                v.body.clone(),
                v.sig.clone(),
                v.pubkey.clone(),
            );
        }));
        assert!(res.is_err(), "a chat_id replay must hit ERR_TEE_REPLAY");
        // The chat id is recorded in used_tee_chat_ids.
        assert!(c.used_tee_chat_ids.contains(&v.chat_id));
    }

    /// QA-114 (TC-TEE-04/05) — payload `strain_id` ≠ target → `ERR_PAYLOAD_STRAIN_MISMATCH`;
    /// unregistered signer → `ERR_SIGNER_NOT_REGISTERED`.
    #[test]
    fn tc_tee_04_payload_mismatch_and_signer() {
        let (mut c, v) = setup_five_strains();
        // The live body payload targets strain 5 conceptually — applied to strain 1 → mismatch.
        // (The embedded vector's payload carries strain_id 4; strain 1 is a different target.)
        ctx("alkemis.testnet", 0, 1_000);
        c.commit_genome_experiment(1, v.req.clone());
        ctx("alkemis.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.apply_tee_mutation_zero_trust(
                1,
                v.req.clone(),
                v.body.clone(),
                v.sig.clone(),
                v.pubkey.clone(),
            );
        }));
        assert!(
            res.is_err(),
            "a payload for another strain must hit ERR_PAYLOAD_STRAIN_MISMATCH"
        );
        // Unregistered signer: a fresh contract without register_attested_signer.
        ctx("alkemis.testnet", 0, 1_000); // SEC-113: ctx before new()
        let mut c2 = SnaccContract::new();
        for _ in 0..5 {
            ctx("alkemis.testnet", 10u128.pow(24), 1_000);
            let _ = c2.incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0);
        }
        ctx("alkemis.testnet", 0, 1_000);
        c2.commit_genome_experiment(4, v.req.clone());
        ctx("alkemis.testnet", 0, 1_000);
        let res2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c2.apply_tee_mutation_zero_trust(
                4,
                v.req.clone(),
                v.body.clone(),
                v.sig.clone(),
                v.pubkey.clone(),
            );
        }));
        assert!(
            res2.is_err(),
            "an unregistered signer must hit ERR_SIGNER_NOT_REGISTERED"
        );
    }

    // ── SEC-113 + DK-070/073/074 — TEE hardening (2026-11-08) ──────────────

    // SEC-113 NOTE: the `#[private]` gate is generated in the EXPORTED WASM WRAPPER
    // (not in the Rust method body), so it cannot be triggered through a direct Rust
    // call in a unit test — it is verified live: a `migrate` call from another
    // account on testnet is rejected (recorded in the deployment report).

    /// SEC-113 — the owner is set at deploy time; the first-caller-becomes-owner
    /// pattern is GONE: any method called by another account does NOT make it the owner.
    #[test]
    fn tc_sec113_first_caller_is_not_owner() {
        ctx("alkemis.testnet", 0, 1_000);
        let mut c = SnaccContract::new();
        assert_eq!(
            c.owner_id,
            Some(test_account("alkemis.testnet")),
            "owner = deployer"
        );
        // Another account registers a signer → rejected; the owner does NOT change.
        ctx("bob.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let v = live_vector();
            c.register_attested_signer(v.pubkey, 1_700_000_000);
        }));
        assert!(res.is_err(), "a non-owner registration must be rejected");
        assert_eq!(
            c.owner_id,
            Some(test_account("alkemis.testnet")),
            "the owner does not change"
        );
    }

    /// SEC-113/DK-070 — `migrate` (private) keeps the owner from the marker.
    #[test]
    fn tc_sec113_owner_kept_after_migrate() {
        ctx("alkemis.testnet", 0, 1_000);
        let _c = SnaccContract::new();
        ctx("snacc-v001.testnet", 0, 1_000); // private: predecessor = the contract account
        let c = SnaccContract::migrate();
        assert_eq!(
            c.owner_id,
            Some(test_account("alkemis.testnet")),
            "owner preserved across migrate"
        );
    }

    /// DK-070 — overwriting a registered key with different attestation is rejected (I-16);
    /// the `signer_added` / `signer_removed` events appear with their exact names.
    #[test]
    fn tc_dk070_signer_events_and_anti_overwrite() {
        ctx("alkemis.testnet", 0, 1_000);
        let mut c = SnaccContract::new();
        let v = live_vector();
        ctx("alkemis.testnet", 0, 1_000);
        c.register_attested_signer(v.pubkey.clone(), 1_700_000_000);
        let logs = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs.contains(r#""kind":"signer_added""#), "event signer_added");
        // Overwriting with DIFFERENT attestation → rejected (I-16, never silent).
        ctx("alkemis.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.register_attested_signer(v.pubkey.clone(), 1_800_000_000);
        }));
        assert!(
            res.is_err(),
            "overwriting an active key with different data must be rejected"
        );
        // Idempotent with the SAME data → allowed.
        ctx("alkemis.testnet", 0, 1_000);
        c.register_attested_signer(v.pubkey.clone(), 1_700_000_000);
        // Remove by a non-owner → rejected.
        ctx("bob.testnet", 0, 1_000);
        let res2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.remove_attested_signer(v.pubkey.clone());
        }));
        assert!(res2.is_err(), "removal by a non-owner must be rejected");
        // Remove by the owner → event signer_removed.
        ctx("alkemis.testnet", 0, 1_000);
        c.remove_attested_signer(v.pubkey.clone());
        let logs2 = near_sdk::test_utils::get_logs().join("\n");
        assert!(logs2.contains(r#""kind":"signer_removed""#), "event signer_removed");
        // The key is gone → a TEE apply with that key is rejected.
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let id = c
            .incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0)
            .strain_id;
        ctx("alkemis.testnet", 0, 1_000);
        c.commit_genome_experiment(id, v.req.clone());
        ctx("alkemis.testnet", 0, 1_000);
        let res3 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.apply_tee_mutation_zero_trust(id, v.req.clone(), v.body, v.sig, v.pubkey);
        }));
        assert!(res3.is_err(), "a removed key is no longer trusted");
    }

    /// DK-073 — an oversized TEE response (4 KB) is rejected BEFORE processing.
    #[test]
    fn tc_dk073_oversized_response_rejected() {
        ctx("alkemis.testnet", 0, 1_000);
        let mut c = SnaccContract::new();
        let v = live_vector();
        ctx("alkemis.testnet", 0, 1_000);
        c.register_attested_signer(v.pubkey.clone(), 1_700_000_000);
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let id = c
            .incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0)
            .strain_id;
        ctx("alkemis.testnet", 0, 1_000);
        c.commit_genome_experiment(id, v.req.clone());
        // A 5 KB body (valid JSON padding shape) → rejected at the size gate.
        let big = format!("{{\"padding\":\"{}\"}}", "x".repeat(5 * 1024));
        ctx("alkemis.testnet", 0, 1_000);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.apply_tee_mutation_zero_trust(id, v.req, big, v.sig, v.pubkey);
        }));
        let msg = res
            .err()
            .and_then(|a| {
                a.downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| a.downcast_ref::<String>().cloned())
            })
            .unwrap_or_default();
        assert!(
            msg.contains("ERR_TEE_RESPONSE_TOO_LARGE"),
            "a response > 4 KB must be rejected at the size gate (DK-073), got: {msg}"
        );
    }

    /// DK-074 — a strict payload schema: unknown fields are rejected
    /// (`deny_unknown_fields`).
    #[test]
    fn tc_dk074_unknown_field_rejected() {
        ctx("alkemis.testnet", 0, 1_000);
        let mut c = SnaccContract::new();
        let v = live_vector();
        ctx("alkemis.testnet", 0, 1_000);
        c.register_attested_signer(v.pubkey.clone(), 1_700_000_000);
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let id = c
            .incubate_strain(StrainRole::Synthesizer, U128(10u128.pow(24)), 0)
            .strain_id;
        ctx("alkemis.testnet", 0, 1_000);
        c.commit_genome_experiment(id, v.req.clone());
        // DK-074 — test the payload parser DIRECTLY (private fn; the parse gate sits
        // after signature verification, so apply() is unreachable without an enclave key).
        let payload_with_unknown = format!(
            r#"{{"strain_id":{id},"efficiency_shift_bps":100,"vitality_bonus":100,"admin_override":true}}"#
        );
        let content_with_unknown = format!(
            r#"{{"id":"x","choices":[{{"message":{{"content":{}}}}}]}}"#,
            near_sdk::serde_json::to_string(&payload_with_unknown).unwrap()
        );
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SnaccContract::parse_mutation_payload(&content_with_unknown, id);
        }));
        let msg = res
            .err()
            .and_then(|a| {
                a.downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| a.downcast_ref::<String>().cloned())
            })
            .unwrap_or_default();
        assert!(
            !msg.is_empty(),
            "an unknown payload field must be rejected (DK-074)"
        );
        // The same payload WITHOUT the unknown field parses fine (proof that the
        // rejection was purely because of the unknown field, not the general shape).
        let clean_payload = format!(
            r#"{{"strain_id":{id},"efficiency_shift_bps":100,"vitality_bonus":100}}"#
        );
        let clean_content = format!(
            r#"{{"id":"x","choices":[{{"message":{{"content":{}}}}}]}}"#,
            near_sdk::serde_json::to_string(&clean_payload).unwrap()
        );
        let p = SnaccContract::parse_mutation_payload(&clean_content, id);
        assert_eq!(p.strain_id, id);
        // DK-074 — a phenotype_trait longer than 32 characters is rejected.
        let payload_long_trait = format!(
            r#"{{"strain_id":{id},"efficiency_shift_bps":100,"vitality_bonus":100,"phenotype_trait":"{}"}}"#,
            "x".repeat(33)
        );
        let content_long_trait = format!(
            r#"{{"id":"x","choices":[{{"message":{{"content":{}}}}}]}}"#,
            near_sdk::serde_json::to_string(&payload_long_trait).unwrap()
        );
        let res2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SnaccContract::parse_mutation_payload(&content_long_trait, id);
        }));
        assert!(res2.is_err(), "a trait > 32 chars must be rejected (DK-074)");
    }
}
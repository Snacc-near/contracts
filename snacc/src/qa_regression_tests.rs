//! Regression tests for the gaps found by the 2026-11-09 backlog reconciliation.
//! TESTS ONLY — no contract logic is changed in this file.
//! Items: SEC-114 (double TEE commit), SEC-118 (adopt/harvest race windows),
//! EN-124 (population fences across 100 division attempts), SEC-104 (cash-invariant property test).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; matches the other qa_* suites
mod qa_regression_tests {
    use crate::state::{constants, StrainRole, StrainStatus};
    use crate::{SnaccContract, StrainId};
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::{AccountId, NearToken};

    const CONTRACT: &str = "snacc-v001.testnet";
    const OWNER: &str = "alkemis.testnet";
    const RAIDER_A: &str = "raider-a.testnet";
    const RAIDER_B: &str = "raider-b.testnet";
    const HUNTER: &str = "hunter.testnet";
    /// One hour in nanoseconds (the NEAR `block_timestamp` unit).
    const HOUR_NS: u64 = 3_600_000_000_000;
    /// Test start: 1:00 h.
    const T0_NS: u64 = HOUR_NS;
    /// A valid 32-byte hex request hash (64 chars) for `commit_genome_experiment`.
    const REQ_SHA_A: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn akun(s: &str) -> AccountId {
        s.parse().unwrap()
    }

    fn ctx(pred: &str, deposit_yocto: u128, ts_ns: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(akun(CONTRACT))
            .signer_account_id(akun(pred))
            .predecessor_account_id(akun(pred))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_ns);
        testing_env!(b.build());
    }

    /// A fresh contract whose clock starts at `T0_NS`.
    fn fresh() -> SnaccContract {
        ctx(OWNER, 0, T0_NS);
        SnaccContract::new()
    }

    /// Incubate with an attached deposit == stake (the recorded decision) at `ts_ns`.
    fn incubate_at(
        c: &mut SnaccContract,
        owner: &str,
        stake: u128,
        role: StrainRole,
        ts_ns: u64,
    ) -> StrainId {
        ctx(owner, stake, ts_ns);
        c.incubate_strain(role, U128(stake), 0).strain_id
    }

    /// Extract the panic payload as text (for `catch_unwind` assertions).
    fn panic_text(err: Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = err.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = err.downcast_ref::<String>() {
            s.clone()
        } else {
            String::new()
        }
    }

    // =====================================================================
    // SEC-114 — double-spend protection for `commit_genome_experiment`
    // =====================================================================

    /// The first commit is accepted and locks the pending commitment.
    #[test]
    fn sec114_first_commit_stores_pending_commitment() {
        let mut c = fresh();
        let id = incubate_at(&mut c, OWNER, 10_000, StrainRole::Extractor, T0_NS);
        ctx(OWNER, 0, T0_NS);
        c.commit_genome_experiment(id, REQ_SHA_A.to_string());
        assert_eq!(
            c.strains.get(&id).unwrap().pending_commitment.as_deref(),
            Some(REQ_SHA_A),
            "the first commit must lock the pending commitment"
        );
    }

    /// A second commit while a commitment is still pending is rejected with
    /// `ERR_COMMITMENT_ALREADY_PENDING` (double-spend of the Phase-1 slot) —
    /// even with the same hash, before the enclave call happens.
    #[test]
    #[should_panic(expected = "ERR_COMMITMENT_ALREADY_PENDING")]
    fn sec114_double_commit_while_pending_rejected() {
        let mut c = fresh();
        let id = incubate_at(&mut c, OWNER, 10_000, StrainRole::Extractor, T0_NS);
        ctx(OWNER, 0, T0_NS);
        c.commit_genome_experiment(id, REQ_SHA_A.to_string()); // accepted
        c.commit_genome_experiment(id, REQ_SHA_A.to_string()); // must be rejected
    }

    // =====================================================================
    // SEC-118 — race/interleaving guards around `adopt` vs `harvest`
    //
    // The implemented guards (read from the code):
    //  * `adopt` checks `target.assert_atomic_ready(expected_target_nonce)` → the
    //    nonce makes concurrent adopters race-proof (exactly one winner);
    //  * the windows are disjoint by design: `adopt` requires `1..=2000`
    //    (`ERR_NOT_DISTRESSED`), `harvest_starving_strain` requires eff == 0
    //    (`ERR_STRAIN_NOT_INSOLVENT`), so no state satisfies both.
    // =====================================================================

    /// Target (strain 1, OWNER, Extractor BMR 100/h → vitality 8500 at `T0_NS`)
    /// + two would-be raiders on separate accounts (strains 2 and 3).
    fn race_setup() -> SnaccContract {
        let mut c = fresh();
        let _ = incubate_at(&mut c, OWNER, 10_000, StrainRole::Extractor, T0_NS); // strain 1: target
        let _ = incubate_at(&mut c, RAIDER_A, 1_000_000, StrainRole::Fermenter, T0_NS); // strain 2
        let _ = incubate_at(&mut c, RAIDER_B, 1_000_000, StrainRole::Fermenter, T0_NS); // strain 3
        c
    }

    /// Two raiders race `adopt` on the same Distressed target: the stale-nonce
    /// racer loses with `ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED` → exactly one winner.
    #[test]
    fn sec118_race_adopt_vs_adopt_stale_nonce_single_winner() {
        let mut c = race_setup();
        // 70 h later the target sits in the Distressed window: 8500 − 70×100 = 1500.
        ctx(RAIDER_A, 0, T0_NS + 70 * HOUR_NS);
        let _ = c.adopt(1, 2, 0); // raider A wins with the current nonce (0)
        assert_eq!(
            c.strains.get(&1).unwrap().state_nonce,
            1,
            "a successful adopt must bump the target nonce"
        );
        assert_eq!(c.strains.get(&1).unwrap().owner_id, akun(RAIDER_A));

        // Raider B races the same target with the stale expected nonce → must lose.
        ctx(RAIDER_B, 0, T0_NS + 70 * HOUR_NS);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.adopt(1, 3, 0)))
            .expect_err("the second adopt must lose the race");
        let msg = panic_text(err);
        assert!(
            msg.contains("ERR_STATE_NONCE_CHANGED_SELLER_DRAIN_BLOCKED"),
            "unexpected error: {msg}"
        );
        // One winner: the target moved once; the loser's own strain is untouched.
        assert_eq!(c.strains.get(&1).unwrap().owner_id, akun(RAIDER_A));
        assert_eq!(c.strains.get(&3).unwrap().owner_id, akun(RAIDER_B));
    }

    /// The adopt window (eff 1..=2000) and the harvest window (eff == 0) are
    /// disjoint: at each state exactly one of the two can win.
    #[test]
    fn sec118_race_adopt_vs_harvest_windows_disjoint() {
        // Tier 2 (eff 1500): harvest CANNOT win, adopt CAN.
        let mut c = race_setup();
        ctx(HUNTER, 0, T0_NS + 70 * HOUR_NS);
        let err =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                c.harvest_starving_strain(1)
            }))
            .expect_err("harvest must lose inside the Distressed window");
        let msg = panic_text(err);
        assert!(msg.contains("ERR_STRAIN_NOT_INSOLVENT"), "unexpected error: {msg}");
        ctx(RAIDER_A, 0, T0_NS + 70 * HOUR_NS);
        let _ = c.adopt(1, 2, 0); // the only viable path here
        assert_eq!(c.strains.get(&1).unwrap().owner_id, akun(RAIDER_A));

        // Tier 3 (eff 0): adopt CANNOT win, harvest CAN.
        let mut c = race_setup();
        ctx(RAIDER_A, 0, T0_NS + 90 * HOUR_NS); // 8500 − 90×100 → saturates to 0
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.adopt(1, 2, 0)))
            .expect_err("adopt must lose at eff == 0");
        let msg = panic_text(err);
        assert!(msg.contains("ERR_NOT_DISTRESSED"), "unexpected error: {msg}");
        ctx(HUNTER, 0, T0_NS + 90 * HOUR_NS);
        let _ = c.harvest_starving_strain(1); // the only viable path here
        assert!(matches!(
            c.strains.get(&1).unwrap().status,
            StrainStatus::Liquidated
        ));
    }

    /// After the harvest wins, the adopter cannot retry: the target is terminal
    /// (`ERR_ALREADY_LIQUIDATED`) — the race ends in exactly one winner.
    #[test]
    fn sec118_race_adopt_after_liquidation_rejected() {
        let mut c = race_setup();
        ctx(HUNTER, 0, T0_NS + 90 * HOUR_NS);
        let _ = c.harvest_starving_strain(1); // harvest wins Tier 3
        ctx(RAIDER_A, 0, T0_NS + 90 * HOUR_NS);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.adopt(1, 2, 0)))
            .expect_err("adopt after liquidation must be rejected");
        let msg = panic_text(err);
        assert!(msg.contains("ERR_ALREADY_LIQUIDATED"), "unexpected error: {msg}");
    }

    // =====================================================================
    // EN-124 — population fences across 100 division attempts
    //
    // Implemented fences (read from the code):
    //  * `divide_strain` rejects at `colony_active_count >= MAX_COLONY_SIZE` (5)
    //    → `ERR_COLONY_FULL` (paid incubation itself is unbounded by the recorded
    //    decision — the fence guards reproduction/adoption, not paid onboarding);
    //  * `child_generation > MAX_GENERATION` (8) → `ERR_MAX_GENERATION`;
    //  * `total_divides > total_mergers + total_lyses` → `ERR_BIRTHS_EXCEEDED`
    //    (the recorded `births ≤ merges + lyses` interpretation).
    // =====================================================================

    fn gen_of(c: &SnaccContract, id: StrainId) -> u8 {
        c.strains
            .get(&id)
            .map(|v| v.generation)
            .unwrap_or(u8::MAX)
    }

    /// Living (not Liquidated/SplicedBurned) strains of `owner`, ascending by id.
    fn living_ids_of(c: &SnaccContract, owner: &str) -> Vec<StrainId> {
        let owner = akun(owner);
        let mut out = Vec::new();
        for id in 1..c.next_strain_id {
            if let Some(v) = c.strains.get(&id) {
                if v.owner_id == owner
                    && !matches!(
                        v.status,
                        StrainStatus::Liquidated | StrainStatus::SplicedBurned
                    )
                {
                    out.push(id);
                }
            }
        }
        out
    }

    /// Test scaffolding: keep ATP above the §13a cost so the ATP assert never
    /// masks the fences under test (ATP accounting itself is covered elsewhere).
    fn top_up_atp(c: &mut SnaccContract, id: StrainId, min: u128) {
        let mut v = c.strains.get(&id).expect("strain must exist").clone();
        if v.atp_balance < min {
            v.atp_balance = min;
            let _ = c.strains.insert(id, v);
        }
    }

    /// The invariants that must hold after EVERY step of the EN-124 loop.
    fn assert_population_fences(c: &SnaccContract) {
        assert!(
            c.macro_state.total_divides
                <= c.macro_state.total_mergers + c.macro_state.total_lyses,
            "births gate breached: divides={} mergers={} lyses={}",
            c.macro_state.total_divides,
            c.macro_state.total_mergers,
            c.macro_state.total_lyses
        );
        for id in 1..c.next_strain_id {
            if let Some(v) = c.strains.get(&id) {
                assert!(
                    v.generation <= constants::MAX_GENERATION,
                    "strain {id} generation {} > MAX_GENERATION {}",
                    v.generation,
                    constants::MAX_GENERATION
                );
            }
        }
    }

    /// EN-124 — 100 division attempts against the population fences: every attempt either
    /// succeeds with the invariants intact, or is rejected by exactly one implemented fence.
    #[test]
    fn en124_loop_100_divide_attempts_never_breach_population_fences() {
        let mut c = fresh();
        const STAKE: u128 = 1_000_000_000_000_000; // 1e15 yocto — halvings inside a ≤8-gen lineage never hit 0
        let mut incubate_seq = 0usize;
        let incubate = |c: &mut SnaccContract, seq: &mut usize| -> StrainId {
            let role = match *seq % 3 {
                0 => StrainRole::Extractor,
                1 => StrainRole::Fermenter,
                _ => StrainRole::Synthesizer,
            };
            *seq += 1;
            incubate_at(c, OWNER, STAKE, role, T0_NS)
        };
        let mut attempts = 0u32;
        let mut successes = 0u32;
        let mut colony_full_rejects = 0u32;
        let mut max_gen_rejects = 0u32;

        // --- Phase 0: push ONE lineage to MAX_GENERATION via 8 splices. ---
        let climber = incubate(&mut c, &mut incubate_seq); // strain 1
        for _ in 0..u32::from(constants::MAX_GENERATION) {
            ctx(OWNER, 0, T0_NS);
            if living_ids_of(&c, OWNER).len() < 2 {
                let _ = incubate(&mut c, &mut incubate_seq); // fresh partner for this splice
            }
            let living = living_ids_of(&c, OWNER);
            let partner = *living.iter().find(|id| **id != climber).expect("partner");
            assert!(
                gen_of(&c, climber) < constants::MAX_GENERATION,
                "climber must stay below MAX before each splice"
            );
            top_up_atp(&mut c, climber, 5); // §13a splice cost
            let _ = c.splice_strains(climber, partner);
            assert_population_fences(&c);
        }
        assert_eq!(gen_of(&c, climber), constants::MAX_GENERATION);

        // --- Phase 0a: MAX_GENERATION fence probe (8 mergers, 0 divides → budget available). ---
        ctx(OWNER, 0, T0_NS);
        top_up_atp(&mut c, climber, 3); // §13a divide cost
        attempts += 1;
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = c.divide_strain(climber);
        }))
        .expect_err("dividing a MAX_GENERATION strain must be rejected");
        let msg = panic_text(err);
        assert!(
            msg.contains("ERR_MAX_GENERATION"),
            "unexpected error at the generation fence: {msg}"
        );
        max_gen_rejects += 1;
        assert_population_fences(&c);

        // --- Phase 0b: COLONY_FULL fence probe (grow to MAX living cells, budget still open). ---
        ctx(OWNER, 0, T0_NS);
        while living_ids_of(&c, OWNER).len() < usize::from(constants::MAX_COLONY_SIZE) {
            let _ = incubate(&mut c, &mut incubate_seq);
        }
        ctx(OWNER, 0, T0_NS);
        let living = living_ids_of(&c, OWNER);
        let probe = living[0];
        top_up_atp(&mut c, probe, 3);
        attempts += 1;
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = c.divide_strain(probe);
        }))
        .expect_err("dividing into a full colony must be rejected");
        let msg = panic_text(err);
        assert!(
            msg.contains("ERR_COLONY_FULL"),
            "unexpected error at the colony fence: {msg}"
        );
        colony_full_rejects += 1;
        assert_population_fences(&c);

        // --- Main loop: drive the attempt counter to 100 with fences checked at every step. ---
        let mut iter = 0u32;
        while attempts < 100 {
            iter += 1;
            assert!(iter < 5_000, "no progress: attempts={attempts}");
            ctx(OWNER, 0, T0_NS); // resets the mock log buffer; same clock
            let living = living_ids_of(&c, OWNER);
            let budget_ok = c.macro_state.total_divides
                < c.macro_state.total_mergers + c.macro_state.total_lyses;

            if !budget_ok {
                // Buy merger budget (the recorded `births` gate input).
                if living.len() < 2 {
                    let _ = incubate(&mut c, &mut incubate_seq);
                    assert_population_fences(&c);
                    continue;
                }
                let dom = living
                    .iter()
                    .copied()
                    .find(|id| gen_of(&c, *id) < constants::MAX_GENERATION);
                let Some(dom) = dom else {
                    let _ = incubate(&mut c, &mut incubate_seq); // fresh gen-0 splice partner
                    assert_population_fences(&c);
                    continue;
                };
                let abs = *living.iter().find(|id| **id != dom).unwrap();
                top_up_atp(&mut c, dom, 5);
                let _ = c.splice_strains(dom, abs);
                assert_population_fences(&c);
                continue;
            }

            if living.len() >= usize::from(constants::MAX_COLONY_SIZE) {
                // Fence probed in phase 0b — free a room via a merger and keep going.
                let dom = living
                    .iter()
                    .copied()
                    .find(|id| gen_of(&c, *id) < constants::MAX_GENERATION);
                let Some(dom) = dom else {
                    let _ = incubate(&mut c, &mut incubate_seq);
                    assert_population_fences(&c);
                    continue;
                };
                let abs = *living.iter().find(|id| **id != dom).unwrap();
                top_up_atp(&mut c, dom, 5);
                let _ = c.splice_strains(dom, abs);
                assert_population_fences(&c);
                continue;
            }

            // Colony has room and the budget is paid → attempt a division.
            let young = living
                .iter()
                .copied()
                .find(|id| gen_of(&c, *id) < constants::MAX_GENERATION);
            let Some(target) = young else {
                let _ = incubate(&mut c, &mut incubate_seq); // every living cell is gen 8
                assert_population_fences(&c);
                continue;
            };
            let before = living.len();
            top_up_atp(&mut c, target, 3);
            attempts += 1;
            let res =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.divide_strain(target)));
            match res {
                Ok(_) => {
                    successes += 1;
                    let after = living_ids_of(&c, OWNER).len();
                    assert_eq!(
                        after,
                        before + 1,
                        "a successful divide must add exactly one living cell"
                    );
                    assert!(
                        before < usize::from(constants::MAX_COLONY_SIZE),
                        "divide must only succeed below MAX_COLONY_SIZE"
                    );
                }
                Err(e) => {
                    let msg = panic_text(e);
                    if msg.contains("ERR_COLONY_FULL") {
                        colony_full_rejects += 1;
                    } else if msg.contains("ERR_MAX_GENERATION") {
                        max_gen_rejects += 1;
                    } else {
                        panic!("unexpected rejection in the main loop: {msg}");
                    }
                }
            }
            assert_population_fences(&c);
        }

        assert_eq!(attempts, 100, "exactly 100 division attempts must be made");
        assert!(successes >= 90, "most attempts should succeed: {successes}");
        assert!(colony_full_rejects >= 1, "the COLONY_FULL fence must fire");
        assert!(max_gen_rejects >= 1, "the MAX_GENERATION fence must fire");
        assert_population_fences(&c);
    }

    // =====================================================================
    // SEC-104 — deterministic property test for the cash invariant V-04:
    // Σ balances + treasury + dividend pool + burned = minted (drift == 0)
    // checked after EVERY step of a 1000-action mixed workload.
    // =====================================================================

    const ALICE: &str = "alice.testnet";
    const BOB: &str = "bob.testnet";
    /// Exactly 10 milliNEAR — above the 5 milliNEAR first-registration minimum.
    const STORAGE_DEPOSIT_YOCTO: u128 = 10_000_000_000_000_000_000_000;

    /// Deterministic xorshift64 PRNG (no new crate; fixed seed → reproducible workload).
    struct XorShift64(u64);

    impl XorShift64 {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        /// Uniform-ish value in `0..n` (`n > 0`).
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// V-04: the ledger must stay exactly solvent after every single action.
    fn assert_cash_invariant(c: &SnaccContract, step: &str, action: usize) {
        let s = c.get_global_solvency();
        assert!(
            s.solvent,
            "{step} (action {action}): not solvent — drift={}" ,
            s.drift
        );
        assert_eq!(
            s.drift, 0,
            "{step} (action {action}): cash invariant V-04 breached"
        );
    }

    /// SEC-104 — 1000 deterministic mixed actions (incubate / feed / burn / transfer /
    /// advance_epoch / claim) with the cash invariant checked after EVERY attempt,
    /// including failed attempts (a partial mutation must never drift either).
    #[test]
    fn sec104_property_1000_mixed_actions_keep_cash_invariant() {
        let mut c = fresh();
        // The workload clock starts 90 h in (after the bounty-funding harvest below).
        let mut now_ns = T0_NS + 90 * HOUR_NS;
        let mut rng = XorShift64(0x5EED_2026_1109);
        let mut ok = [0u32; 6];

        // ---- Warmup: one guaranteed success per action kind (still invariant-checked). ----
        // Fund ALICE through a REAL flow first — liquidating a starved victim pays a
        // 10% bounty to `account_balances[ALICE]`. (Direct ledger seeding would break
        // V-04 by definition: unbacked money is not minted money.)
        let victim = incubate_at(&mut c, OWNER, 10_000, StrainRole::Extractor, T0_NS);
        // Register the transfer counterparties (NEAR storage only — no ledger impact).
        ctx(ALICE, STORAGE_DEPOSIT_YOCTO, T0_NS);
        let _ = c.storage_deposit(None);
        ctx(BOB, STORAGE_DEPOSIT_YOCTO, T0_NS);
        let _ = c.storage_deposit(None);
        assert_cash_invariant(&c, "warmup registration", 3);
        // At `now_ns` (90 h in) the victim is insolvent (8500 − 90×100 → 0) → ALICE liquidates it.
        ctx(ALICE, 0, now_ns);
        let _ = c.harvest_starving_strain(victim);
        assert!(
            c.account_balance(&akun(ALICE)) > 0,
            "the 10% liquidation bounty must fund ALICE"
        );
        assert_cash_invariant(&c, "warmup harvest bounty", 2);

        // 0 · incubate at the new clock (Synthesizer — its caste can redeem aether below).
        const WARM_STAKE: u128 = 1_000_000_000;
        ctx(OWNER, WARM_STAKE, now_ns);
        let sid1 = c
            .incubate_strain(StrainRole::Synthesizer, U128(WARM_STAKE), 0)
            .strain_id;
        ok[0] += 1;
        assert_cash_invariant(&c, "warmup incubate", 0);

        // 1 · feed (accrues the epoch dividend inflow — 50% of the fee, `ADR-28`).
        ctx(OWNER, 0, now_ns);
        let _ = c.feed_strain(sid1, U128(100_000));
        ok[1] += 1;
        assert_cash_invariant(&c, "warmup feed", 1);

        // 2 · ft_burn (owner-only — `SEC-101`).
        ctx(OWNER, 0, now_ns);
        c.ft_burn(sid1, U128(100));
        ok[2] += 1;
        assert_cash_invariant(&c, "warmup ft_burn", 2);

        // 3 · ft_transfer (registered peer, exactly 1 yoctoNEAR; ≤ ALICE's bounty balance).
        ctx(ALICE, 1, now_ns);
        c.ft_transfer(akun(BOB), U128(100), None);
        ok[3] += 1;
        assert_cash_invariant(&c, "warmup ft_transfer", 3);

        // Claim chain: inject aether (test scaffolding) → redeem → close the epoch → claim.
        let _ = c
            .inventories
            .insert((sid1, constants::ELIXIR_AETHER.to_string()), 5);
        ctx(OWNER, 0, now_ns);
        c.synthesize_and_redeem_aether(sid1, U128(1));
        assert_cash_invariant(&c, "warmup synthesize_and_redeem_aether", 5);

        // 4 · advance_epoch (26 h later — the W6 24 h gate passes).
        now_ns += 26 * HOUR_NS;
        ctx(OWNER, 0, now_ns);
        c.advance_epoch();
        ok[4] += 1;
        assert_cash_invariant(&c, "warmup advance_epoch", 4);

        // 5 · claim the closed epoch 0.
        ctx(OWNER, 0, now_ns);
        let payout = c.claim_industrial_dividend(sid1, 0);
        assert!(payout.0 > 0, "the warmup claim must pay out");
        ok[5] += 1;
        assert_cash_invariant(&c, "warmup claim", 5);

        // ---- Property loop: 1000 mixed actions, invariant after every attempt. ----
        for step in 0..1_000usize {
            let action = rng.below(6) as usize;
            let success = match action {
                // incubate
                0 => {
                    let stake = 1_000_000u128 + rng.below(5_000_000_000) as u128;
                    let role = match rng.below(3) {
                        0 => StrainRole::Extractor,
                        1 => StrainRole::Fermenter,
                        _ => StrainRole::Synthesizer,
                    };
                    ctx(OWNER, stake, now_ns);
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        c.incubate_strain(role, U128(stake), 0)
                    }))
                    .is_ok()
                }
                // feed
                1 => {
                    let ids = living_ids_of(&c, OWNER);
                    if ids.is_empty() {
                        false
                    } else {
                        let id = ids[rng.below(ids.len() as u64) as usize];
                        let amount = 1 + rng.below(500) as u128;
                        ctx(OWNER, 0, now_ns);
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            c.feed_strain(id, U128(amount))
                        }))
                        .is_ok()
                    }
                }
                // ft_burn
                2 => {
                    let ids = living_ids_of(&c, OWNER);
                    if ids.is_empty() {
                        false
                    } else {
                        let id = ids[rng.below(ids.len() as u64) as usize];
                        let bal = c.snacc_balances.get(&id).copied().unwrap_or(0);
                        if bal == 0 {
                            false
                        } else {
                            let amount = 1 + rng.below(bal.min(1_000) as u64) as u128;
                            ctx(OWNER, 0, now_ns);
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                c.ft_burn(id, U128(amount))
                            }))
                            .is_ok()
                        }
                    }
                }
                // ft_transfer between the registered peers
                3 => {
                    let (from, to) = if rng.below(2) == 0 {
                        (ALICE, BOB)
                    } else {
                        (BOB, ALICE)
                    };
                    let bal = c.account_balance(&akun(from));
                    if bal == 0 {
                        false
                    } else {
                        let amount = 1 + rng.below(bal.min(1_000) as u64) as u128;
                        ctx(from, 1, now_ns); // exactly 1 yoctoNEAR
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            c.ft_transfer(akun(to), U128(amount), None)
                        }))
                        .is_ok()
                    }
                }
                // advance_epoch after a real 26 h mock-clock jump (the W6 gate)
                4 => {
                    now_ns += 26 * HOUR_NS;
                    ctx(OWNER, 0, now_ns);
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.advance_epoch()))
                        .is_ok()
                }
                // claim a random closed epoch for a random living strain
                _ => {
                    let closed = c.macro_state.last_closed_epoch;
                    let ids = living_ids_of(&c, OWNER);
                    if closed == 0 || ids.is_empty() {
                        false
                    } else {
                        let epoch = rng.below(closed);
                        let id = ids[rng.below(ids.len() as u64) as usize];
                        ctx(OWNER, 0, now_ns);
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            c.claim_industrial_dividend(id, epoch)
                        }))
                        .is_ok()
                    }
                }
            };
            if success {
                ok[action] += 1;
            }
            // The invariant must hold after every attempt — successful OR failed.
            assert_cash_invariant(&c, &format!("step {step}"), action);
        }

        // Every action kind must actually succeed at least once (deterministic seed).
        assert!(
            ok.iter().all(|&n| n > 0),
            "every action kind needs at least one success: {ok:?}"
        );
        assert_cash_invariant(&c, "final", 99);
    }
}
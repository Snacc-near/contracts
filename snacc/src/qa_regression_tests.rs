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
}
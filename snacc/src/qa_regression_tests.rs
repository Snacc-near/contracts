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
}
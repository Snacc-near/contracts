//! QA gap A-06 / EN-117 — ATP enters `V_epoch` (`atp_minted × ATP_VOLUME_SCALE`).
#[cfg(test)]
#[allow(clippy::module_inception)] // module name = file name qa_*; kept so the test paths do not change
mod qa_en117_tests {
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    fn ctx(pred: &str, dep: u128, ts: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id("snacc-v001.testnet".parse().unwrap())
            .signer_account_id(pred.parse().unwrap())
            .predecessor_account_id(pred.parse().unwrap())
            .attached_deposit(NearToken::from_yoctonear(dep))
            .block_timestamp(ts * 1_000_000_000);
        testing_env!(b.build());
    }

    /// `EN-117` — feeding adds at least `atp_minted × ATP_VOLUME_SCALE` to `volume_epoch`.
    #[test]
    fn tc_en117_atp_enters_v_epoch() {
        ctx("alkemis.testnet", 10u128.pow(24), 1_000);
        let mut c = SnaccContract::new();
        let v = c.incubate_strain(StrainRole::Extractor, U128(10u128.pow(24)), 10);
        let atp0 = v.atp_balance;
        let vol0 = c.get_macro_state().volume_epoch;
        ctx("alkemis.testnet", 0, 1_000);
        let v2 = c.feed_strain(v.strain_id, U128(5 * 10u128.pow(17)));
        let minted = v2.atp_balance - atp0;
        assert!(minted > 0, "feeding must mint ATP");
        let vol = c.get_macro_state().volume_epoch;
        let scale = crate::state::constants::ATP_VOLUME_SCALE;
        assert!(
            vol >= vol0 + minted * scale,
            "V_epoch ({vol}) must be >= vol0 + atp_minted({minted}) x {scale}"
        );
    }
}

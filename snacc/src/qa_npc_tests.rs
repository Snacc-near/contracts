//! QA-134..137 (`TC-NPC-*`) — Autonomous AI Alchemist NPCs Test Suite (ADR-30 & I-15):
//! - QA-134: Test NPC quote determinism, scarcity bonding curve P_base * S_target / S_after,
//!   pure view isolation, and temperature telemetry reflection.
//! - QA-135: Test `npc_buy_spores` atomic execution, Rule I-15 exact conservation
//!   (-Δbalance == +Δreserve == total_cost, -Δnpc_stock == +Δinventory == buy_amount),
//!   state_nonce increment (ADR-29), trades counter, and NEP-297 event emission.
//! - QA-136: Negative validation gates (ERR_BAD_AMOUNT, ERR_NPC_OUT_OF_STOCK,
//!   ERR_NOT_OWNED_BY_CALLEE, ERR_INSUFFICIENT_BALANCE, ERR_STRAIN_NOT_FOUND, ERR_NPC_UNKNOWN).
//! - QA-137: Sequential multi-trade compounding curve, cross-NPC independent liquidity,
//!   reasoning_tooltip verbatim audit, and global solvency conservation (drift == 0).

#[cfg(test)]
#[allow(clippy::module_inception)]
mod qa_npc_tests {
    use crate::state::constants;
    use crate::state::StrainRole;
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::{get_logs, VMContextBuilder};
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    const ALKEMIS: &str = "alkemis.near";
    const RAIDER: &str = "raider.near";
    const CONTRACT_ID: &str = "snacc-v001.near";
    const T0: u64 = 1_000;
    const INITIAL_STAKE: u128 = 100_000;

    fn acct(s: &str) -> near_sdk::AccountId {
        s.parse().unwrap()
    }

    fn ctx(predecessor: &str, deposit_yocto: u128, ts_sec: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acct(CONTRACT_ID))
            .signer_account_id(acct(predecessor))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(ts_sec * 1_000_000_000);
        testing_env!(b.build());
    }

    fn fresh_contract() -> SnaccContract {
        ctx(ALKEMIS, 0, T0);
        let mut c = SnaccContract::new();
        ctx(ALKEMIS, INITIAL_STAKE, T0);
        // Incubate strain #1: 80% to strain balance (80,000), 20% to treasury
        let _ = c.incubate_strain(StrainRole::Extractor, U128(INITIAL_STAKE), 10);
        c
    }

    fn set_temp(c: &mut SnaccContract, temp_mk: u64, ts_sec: u64) {
        let supply = c.macro_state.total_snacc_supply;
        c.macro_state.volume_epoch = u128::from(temp_mk) * supply / 1000;
        c.macro_state.last_update_ms = ts_sec * 1000;
    }

    fn get_strain_snacc(c: &SnaccContract, strain_id: u64) -> u128 {
        c.snacc_balances.get(&strain_id).copied().unwrap_or(0)
    }

    // =========================================================================
    // QA-134: NPC Quote Determinism, Scarcity Curve & Temperature Telemetry
    // =========================================================================

    #[test]
    fn tc_npc_01_quote_scarcity_formula_all_three_archetypes() {
        let c = fresh_contract();

        // 1. Pyre-X (npc-extractor): S_target = 500, P_base = 100
        // Buy 1: s_after = 499 -> floor(100 * 500 / 499) = 100 -> total 100
        let q1 = c.npc_quote_spores("npc-extractor".to_string(), 1);
        assert_eq!(q1.unit_price_vita, 100);
        assert_eq!(q1.total_cost_snacc, 100);
        assert_eq!(q1.npc_spores_stock_after, 499);

        // Buy 100: s_after = 400 -> floor(100 * 500 / 400) = 125 -> total 12,500
        let q2 = c.npc_quote_spores("npc-extractor".to_string(), 100);
        assert_eq!(q2.unit_price_vita, 125);
        assert_eq!(q2.total_cost_snacc, 12_500);
        assert_eq!(q2.npc_spores_stock_after, 400);

        // Buy 250: s_after = 250 -> floor(100 * 500 / 250) = 200 -> total 50,000
        let q3 = c.npc_quote_spores("npc-extractor".to_string(), 250);
        assert_eq!(q3.unit_price_vita, 200);
        assert_eq!(q3.total_cost_snacc, 50_000);
        assert_eq!(q3.npc_spores_stock_after, 250);

        // Buy 400: s_after = 100 -> floor(100 * 500 / 100) = 500 -> total 200,000
        let q4 = c.npc_quote_spores("npc-extractor".to_string(), 400);
        assert_eq!(q4.unit_price_vita, 500);
        assert_eq!(q4.total_cost_snacc, 200_000);
        assert_eq!(q4.npc_spores_stock_after, 100);

        // 2. Myco-7 (npc-central-arb): S_target = 1000, P_base = 100
        // Buy 100: s_after = 900 -> floor(100 * 1000 / 900) = 111 -> total 11,100
        let q_arb1 = c.npc_quote_spores("npc-central-arb".to_string(), 100);
        assert_eq!(q_arb1.unit_price_vita, 111);
        assert_eq!(q_arb1.total_cost_snacc, 11_100);
        assert_eq!(q_arb1.npc_spores_stock_after, 900);

        // Buy 500: s_after = 500 -> floor(100 * 1000 / 500) = 200 -> total 100,000
        let q_arb2 = c.npc_quote_spores("npc-central-arb".to_string(), 500);
        assert_eq!(q_arb2.unit_price_vita, 200);
        assert_eq!(q_arb2.total_cost_snacc, 100_000);
        assert_eq!(q_arb2.npc_spores_stock_after, 500);

        // 3. Void-0 (npc-vulture-ma): S_target = 300, P_base = 100
        // Buy 1: s_after = 299 -> floor(100 * 300 / 299) = 100 -> total 100
        let q_vul1 = c.npc_quote_spores("npc-vulture-ma".to_string(), 1);
        assert_eq!(q_vul1.unit_price_vita, 100);
        assert_eq!(q_vul1.total_cost_snacc, 100);
        assert_eq!(q_vul1.npc_spores_stock_after, 299);

        // Buy 150: s_after = 150 -> floor(100 * 300 / 150) = 200 -> total 30,000
        let q_vul2 = c.npc_quote_spores("npc-vulture-ma".to_string(), 150);
        assert_eq!(q_vul2.unit_price_vita, 200);
        assert_eq!(q_vul2.total_cost_snacc, 30_000);
        assert_eq!(q_vul2.npc_spores_stock_after, 150);

        // Buy 299: s_after = 1 -> floor(100 * 300 / 1) = 30,000 -> total 8,970,000
        let q_vul3 = c.npc_quote_spores("npc-vulture-ma".to_string(), 299);
        assert_eq!(q_vul3.unit_price_vita, 30_000);
        assert_eq!(q_vul3.total_cost_snacc, 30_000 * 299);
        assert_eq!(q_vul3.npc_spores_stock_after, 1);
    }

    #[test]
    fn tc_npc_02_quote_determinism_and_temperature_reflection() {
        let mut c = fresh_contract();

        // Check determinism: multiple quotes yield identical outputs
        let q1 = c.npc_quote_spores("npc-extractor".to_string(), 50);
        let q2 = c.npc_quote_spores("npc-extractor".to_string(), 50);
        assert_eq!(q1.unit_price_vita, q2.unit_price_vita);
        assert_eq!(q1.total_cost_snacc, q2.total_cost_snacc);
        assert_eq!(q1.reasoning_tooltip, q2.reasoning_tooltip);

        // View isolation: verify state_nonce and balances remain unchanged by quote views
        let strain = c.get_strain(1);
        assert_eq!(strain.state_nonce, 0);
        let bal = get_strain_snacc(&c, 1);
        assert_eq!(bal, 80_000);

        // Temperature reflection in quote: Normal -> Fever -> Hypothermia
        set_temp(&mut c, 605, T0);
        let q_norm = c.npc_quote_spores("npc-extractor".to_string(), 10);
        assert_eq!(q_norm.crucible_temp_mk, 605);

        set_temp(&mut c, 630, T0 + 10);
        let q_fever = c.npc_quote_spores("npc-extractor".to_string(), 10);
        assert_eq!(q_fever.crucible_temp_mk, 630);

        set_temp(&mut c, 580, T0 + 20);
        let q_hypo = c.npc_quote_spores("npc-extractor".to_string(), 10);
        assert_eq!(q_hypo.crucible_temp_mk, 580);
    }

    // =========================================================================
    // QA-135: `npc_buy_spores` State Mutation & Rule I-15 Conservation
    // =========================================================================

    #[test]
    fn tc_npc_03_buy_spores_atomic_conservation_i15() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);

        // Pre-conditions
        let bal_before = get_strain_snacc(&c, 1);
        assert_eq!(bal_before, 80_000);
        let npc_before = c.get_npc("npc-extractor".to_string());
        assert_eq!(npc_before.spores_stock, 500);
        assert_eq!(npc_before.snacc_reserve, 0);
        assert_eq!(npc_before.trades_executed, 0);
        let sum_reserve_before = c.macro_state.sum_npc_reserve;
        assert_eq!(sum_reserve_before, 0);

        // Buy 50 spores: s_after = 450 -> unit = floor(100 * 500 / 450) = 111 -> total = 5,550
        let quote = c.npc_buy_spores(1, "npc-extractor".to_string(), 50);
        assert_eq!(quote.unit_price_vita, 111);
        assert_eq!(quote.total_cost_snacc, 5_550);
        assert_eq!(quote.npc_spores_stock_after, 450);

        // Post-conditions: Rule I-15 exact conservation
        let bal_after = get_strain_snacc(&c, 1);
        assert_eq!(bal_after, bal_before - 5_550);

        let npc_after = c.get_npc("npc-extractor".to_string());
        assert_eq!(npc_after.spores_stock, 450);
        assert_eq!(npc_after.snacc_reserve, 5_550);
        assert_eq!(npc_after.trades_executed, 1);

        assert_eq!(c.macro_state.sum_npc_reserve, 5_550);

        // Strain inventory credited with 50 Spore-Ignis (10 initial from incubation + 50 bought = 60)
        let spore_balance = c.inventories.get(&(1, constants::GENESIS_SPORE_TOKEN_ID.to_string())).copied().unwrap_or(0);
        assert_eq!(spore_balance, 60);

        // ADR-29 anti-frontrunning nonce increment
        let strain_after = c.get_strain(1);
        assert_eq!(strain_after.state_nonce, 1);

        // Event emission verification
        let logs = get_logs().join("\n");
        assert!(logs.contains(r#""kind":"npc_spores_bought""#));
        assert!(logs.contains(r#""npc_id":"npc-extractor""#));
        assert!(logs.contains(r#""amount":50"#));
        assert!(logs.contains(r#""unit_price":111"#));
        assert!(logs.contains(r#""total_cost":5550"#));
    }

    // =========================================================================
    // QA-136: Negative Assertions, Panics & Guardrails
    // =========================================================================

    #[test]
    #[should_panic(expected = "ERR_BAD_AMOUNT")]
    fn tc_npc_04_panic_quote_zero_amount() {
        let c = fresh_contract();
        let _ = c.npc_quote_spores("npc-extractor".to_string(), 0);
    }

    #[test]
    #[should_panic(expected = "ERR_BAD_AMOUNT")]
    fn tc_npc_05_panic_buy_zero_amount() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 0);
    }

    #[test]
    #[should_panic(expected = "ERR_NPC_UNKNOWN")]
    fn tc_npc_06_panic_unknown_npc_id_quote() {
        let c = fresh_contract();
        let _ = c.npc_quote_spores("npc-phantom".to_string(), 10);
    }

    #[test]
    #[should_panic(expected = "ERR_NPC_UNKNOWN")]
    fn tc_npc_07_panic_unknown_npc_id_buy() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);
        let _ = c.npc_buy_spores(1, "npc-phantom".to_string(), 10);
    }

    #[test]
    #[should_panic(expected = "ERR_NPC_OUT_OF_STOCK")]
    fn tc_npc_08_panic_out_of_stock() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);
        // Pyre-X has 500 spores stock initially; buying 501 must panic
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 501);
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_OWNED_BY_CALLEE")]
    fn tc_npc_09_panic_non_owner_attempted_buy() {
        let mut c = fresh_contract();
        // Strain #1 is owned by ALKEMIS. RAIDER calls npc_buy_spores
        ctx(RAIDER, 0, T0);
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 10);
    }

    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_BALANCE")]
    fn tc_npc_10_panic_insufficient_strain_balance() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);
        // Strain balance is 80,000.
        // Buying 480 spores from Pyre-X: s_after = 20 -> unit = floor(100 * 500 / 20) = 2,500
        // total_cost = 2,500 * 480 = 1,200,000 > 80,000 balance -> must panic
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 480);
    }

    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn tc_npc_11_panic_strain_not_found() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);
        let _ = c.npc_buy_spores(999, "npc-extractor".to_string(), 10);
    }

    // =========================================================================
    // QA-137: Sequential Multi-Trade Curve, Tooltip & Global Solvency
    // =========================================================================

    #[test]
    fn tc_npc_12_sequential_trading_dynamic_bonding_curve() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);

        // We execute 4 successive buys of 50 spores from Pyre-X:
        // Stock initially: 500
        // Trade 1: buy 50 -> s_after = 450 -> unit = floor(50000/450) = 111 -> cost 5,550
        let q1 = c.npc_buy_spores(1, "npc-extractor".to_string(), 50);
        assert_eq!(q1.unit_price_vita, 111);
        assert_eq!(q1.total_cost_snacc, 5_550);
        assert_eq!(q1.npc_spores_stock_after, 450);

        // Trade 2: buy 50 -> stock was 450 -> s_after = 400 -> unit = floor(50000/400) = 125 -> cost 6,250
        let q2 = c.npc_buy_spores(1, "npc-extractor".to_string(), 50);
        assert_eq!(q2.unit_price_vita, 125);
        assert_eq!(q2.total_cost_snacc, 6_250);
        assert_eq!(q2.npc_spores_stock_after, 400);

        // Trade 3: buy 50 -> stock was 400 -> s_after = 350 -> unit = floor(50000/350) = 142 -> cost 7,100
        let q3 = c.npc_buy_spores(1, "npc-extractor".to_string(), 50);
        assert_eq!(q3.unit_price_vita, 142);
        assert_eq!(q3.total_cost_snacc, 7_100);
        assert_eq!(q3.npc_spores_stock_after, 350);

        // Trade 4: buy 50 -> stock was 350 -> s_after = 300 -> unit = floor(50000/300) = 166 -> cost 8,300
        let q4 = c.npc_buy_spores(1, "npc-extractor".to_string(), 50);
        assert_eq!(q4.unit_price_vita, 166);
        assert_eq!(q4.total_cost_snacc, 8_300);
        assert_eq!(q4.npc_spores_stock_after, 300);

        // Cumulative assertions
        let npc = c.get_npc("npc-extractor".to_string());
        let expected_total_cost = 5_550 + 6_250 + 7_100 + 8_300;
        assert_eq!(npc.snacc_reserve, expected_total_cost);
        assert_eq!(npc.spores_stock, 300);
        assert_eq!(npc.trades_executed, 4);

        let strain = c.get_strain(1);
        assert_eq!(strain.state_nonce, 4);
        assert_eq!(get_strain_snacc(&c, 1), 80_000 - expected_total_cost);

        // Spore inventory balance equals 210 (10 initial from incubation + 200 bought)
        let inv = c.inventories.get(&(1, constants::GENESIS_SPORE_TOKEN_ID.to_string())).copied().unwrap_or(0);
        assert_eq!(inv, 210);

        // Solvency check
        let sol = c.get_global_solvency();
        assert!(sol.solvent, "Protocol must remain solvent after sequential trades");
        assert_eq!(sol.drift, 0, "Drift must strictly be 0 yocto");
    }

    #[test]
    fn tc_npc_13_cross_npc_independent_liquidity() {
        let mut c = fresh_contract();
        ctx(ALKEMIS, 0, T0);

        // Trade with Pyre-X
        let _ = c.npc_buy_spores(1, "npc-extractor".to_string(), 100);

        // Verify Myco-7 and Void-0 were completely unaffected
        let arb = c.get_npc("npc-central-arb".to_string());
        assert_eq!(arb.spores_stock, 1000);
        assert_eq!(arb.snacc_reserve, 0);
        assert_eq!(arb.trades_executed, 0);

        let vulture = c.get_npc("npc-vulture-ma".to_string());
        assert_eq!(vulture.spores_stock, 300);
        assert_eq!(vulture.snacc_reserve, 0);
        assert_eq!(vulture.trades_executed, 0);
    }

    #[test]
    fn tc_npc_14_reasoning_tooltip_verbatim_validation() {
        let c = fresh_contract();

        let q = c.npc_quote_spores("npc-extractor".to_string(), 50);
        let expected_text = "I hold 500 Spore-Ignis (target 500). Selling 50 drops my stock to 450 — unit price rises to 111 (P_base 100). Scarcity is my leverage.";
        assert_eq!(q.reasoning_tooltip, expected_text);

        let q_vul = c.npc_quote_spores("npc-vulture-ma".to_string(), 100);
        let expected_vul = "I hold 300 Spore-Ignis (target 300). Selling 100 drops my stock to 200 — unit price rises to 150 (P_base 100). Scarcity is my leverage.";
        assert_eq!(q_vul.reasoning_tooltip, expected_vul);
    }
}

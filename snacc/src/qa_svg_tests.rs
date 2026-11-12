//! QA-127..129 (`TC-SVG-*`) — Feature 7 test suite (On-Chain Telemetry, SVG & Appetite):
//! - QA-127: SVG generator determinism, size limit <= 1734 bytes, 8 real-time indicators,
//!   and solvency states (ALIVE vs dark red `#ff2255` LIQUIDATED).
//! - QA-128: `get_appetite_meter()` telemetry view (zone, temperature, burst countdown,
//!   dormancy status, Great Feast progress, and risk heat).
//! - QA-129: `zone_changed` NEP-297 event emission upon macroeconomic zone transitions
//!   (NORMAL <-> FEVER <-> HYPO) without spurious duplicate emissions within the same zone.
#[cfg(test)]
#[allow(clippy::module_inception)]
mod qa_svg_tests {
    use crate::state::StrainRole;
    use crate::svg;
    use crate::SnaccContract;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::{get_logs, VMContextBuilder};
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    const ALKEMIS: &str = "alkemis.near";
    const CONTRACT_ID: &str = "snacc-v001.near";
    const T0: u64 = 1_000;
    const STAKE: u128 = 100_000;

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
        ctx(ALKEMIS, STAKE, T0);
        let _ = c.incubate_strain(StrainRole::Extractor, U128(STAKE), 10);
        c.macro_state.params.dormancy_threshold_bps = 0;
        c
    }

    fn set_temp(c: &mut SnaccContract, temp_mk: u64, ts_sec: u64) {
        let supply = c.macro_state.total_snacc_supply;
        c.macro_state.volume_epoch = u128::from(temp_mk) * supply / 1000;
        c.macro_state.last_update_ms = ts_sec * 1000;
    }

    fn decode_base64_data_uri(uri: &str) -> String {
        const PREFIX: &str = "data:image/svg+xml;base64,";
        assert!(uri.starts_with(PREFIX), "URI must start with Base64 SVG prefix");
        let b64 = &uri[PREFIX.len()..];
        // Standard self-contained decoding
        let decoded = base64_decode(b64);
        String::from_utf8(decoded).expect("Decoded bytes must be valid UTF-8 SVG")
    }

    fn base64_decode(input: &str) -> Vec<u8> {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut table = [255u8; 256];
        for (i, &b) in T.iter().enumerate() {
            table[b as usize] = i as u8;
        }

        let mut out = Vec::new();
        let bytes = input.as_bytes();
        let mut buf = 0u32;
        let mut bits = 0;

        for &b in bytes {
            if b == b'=' || b == b'\r' || b == b'\n' || b == b' ' {
                continue;
            }
            let val = table[b as usize];
            if val == 255 {
                continue;
            }
            buf = (buf << 6) | (val as u32);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
                buf &= (1 << bits) - 1;
            }
        }
        out
    }

    // =========================================================================
    // QA-127 (TC-SVG-01/02): SVG Determinism, Size Bound, 8 Indicators, Solvency
    // =========================================================================

    /// QA-127: The SVG string produced is deterministic and never exceeds 1,734 bytes.
    #[test]
    fn tc_svg_01_deterministic_and_upper_bound_1734_bytes() {
        let cases = [
            // Standard alive strain
            (1u64, 0b001u8, 0u8, 80_000u128, 50u128, 0u8, 0u8, 0u8, 0u64, 8_000u128, 605u64, "NORMAL", false),
            // Max parameter strain with high digits
            (999, 0b111, 8, 9_999_999_999_999, 999_999, 5, 5, 5, 86_400, 999_999_999, 10_000, "FEVER", false),
            // Liquidated strain
            (42, 0b010, 2, 0, 0, 1, 2, 0, 120, 0, 300, "HYPO", true),
        ];

        for (id, role, gen, snacc, atp, mito, nuk, liso, hb, bounty, temp, zone, liq) in cases {
            let svg1 = svg::render_strain_svg(id, role, gen, snacc, atp, mito, nuk, liso, hb, bounty, temp, zone, liq);
            let svg2 = svg::render_strain_svg(id, role, gen, snacc, atp, mito, nuk, liso, hb, bounty, temp, zone, liq);

            assert_eq!(svg1, svg2, "SVG rendering must be strictly deterministic");
            assert!(
                svg1.len() <= 1_734,
                "SVG size {} bytes exceeds canonical upper bound of 1,734 bytes",
                svg1.len()
            );
            assert!(svg1.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
            assert!(svg1.ends_with(r#"</svg>"#));
        }
    }

    /// QA-127: Verify presence of all 8 telemetry indicators in the rendered SVG.
    #[test]
    fn tc_svg_01_eight_realtime_indicators_present() {
        let mut c = fresh_contract();
        set_temp(&mut c, 605, T0);

        let token_view = c.nft_token("1".to_string());
        assert_eq!(token_view.token_id, "1");
        assert_eq!(token_view.owner_id, acct(ALKEMIS));
        assert!(token_view.metadata.title.contains("Snacc Strain #1"));

        let svg_text = decode_base64_data_uri(&token_view.metadata.media);

        // 1. Identity & Role/Generation
        assert!(svg_text.contains("SNACC #1 R1 G0"), "Missing strain identity chip");
        // 2. Heartbeat (elapsed since last feed)
        assert!(svg_text.contains("♥ 0s"), "Missing heartbeat indicator");
        // 3. Bounty (liquidator bounty)
        assert!(svg_text.contains("☠ 8000"), "Missing bounty indicator");
        // 4. SNACC balance
        assert!(svg_text.contains("💎 80000"), "Missing SNACC balance indicator");
        // 5. ATP internal energy
        assert!(svg_text.contains("⚡ 50 ATP"), "Missing ATP balance indicator");
        // 6. Organelle levels (M, N, L)
        assert!(svg_text.contains("🧬 M0 N0 L0"), "Missing organelle slots indicator");
        // 7. Macro temperature & Zone
        assert!(svg_text.contains("🌡 605 mK [NORMAL]"), "Missing temperature & zone indicator");
        // 8. Solvency status
        assert!(svg_text.contains("ALIVE"), "Missing ALIVE status indicator");
        assert!(svg_text.contains(r##"fill="#0b0f1a""##), "Alive background color must be #0b0f1a");
    }

    /// QA-127: Liquidated status renders dark red `#ff2255` background and `STATUS: LIQUIDATED`.
    #[test]
    fn tc_svg_02_liquidated_renders_red_status() {
        let mut c = fresh_contract();
        // Force strain #1 to liquidated
        let mut v = c.strains.get(&1).unwrap().clone();
        v.status = crate::state::StrainStatus::Liquidated;
        let _ = c.strains.insert(1, v);

        let token_view = c.nft_token("1".to_string());
        let svg_text = decode_base64_data_uri(&token_view.metadata.media);

        assert!(svg_text.contains(r##"fill="#ff2255""##), "Must render dark red background");
        assert!(svg_text.contains("STATUS: LIQUIDATED"), "Must render STATUS: LIQUIDATED text");
    }

    /// QA-127: nft_token with unknown id panics with ERR_STRAIN_NOT_FOUND.
    #[test]
    #[should_panic(expected = "ERR_STRAIN_NOT_FOUND")]
    fn tc_svg_02_unknown_token_id_panics() {
        let c = fresh_contract();
        let _ = c.nft_token("9999".to_string());
    }

    // =========================================================================
    // QA-128 (TC-SVG-03): get_appetite_meter Telemetry View
    // =========================================================================

    /// QA-128: `get_appetite_meter` mirrors macro state, temperature, zones, burst countdown,
    /// dormancy, feast volume, feast target, and risk heat.
    #[test]
    fn tc_svg_03_appetite_meter_telemetry_accuracy() {
        let mut c = fresh_contract();

        // 1. Initial State: supply = 100_000, volume = 0 -> temp = 0 -> HYPO
        set_temp(&mut c, 0, T0);
        let m0 = c.get_appetite_meter();
        assert_eq!(m0.zone, "HYPO");
        assert_eq!(m0.crucible_temp, 0);
        assert_eq!(m0.current_epoch, 0);
        assert_eq!(m0.burst_bps, 0);
        assert_eq!(m0.burst_until, 0);
        assert_eq!(m0.burst_epochs_left, 0);
        assert_eq!(m0.feast_volume, 0);
        // Feast target = 5% of 100_000 = 5_000
        assert_eq!(m0.feast_target, 5_000);

        // 2. Normal Zone Band (600..=610)
        set_temp(&mut c, 605, T0);
        let m1 = c.get_appetite_meter();
        assert_eq!(m1.zone, "NORMAL");
        assert_eq!(m1.crucible_temp, 605);

        // 3. Fever Zone (>610)
        set_temp(&mut c, 850, T0);
        let m2 = c.get_appetite_meter();
        assert_eq!(m2.zone, "FEVER");
        assert_eq!(m2.crucible_temp, 850);

        // 4. Nutrient Burst Active Countdown
        c.macro_state.nutrient_burst_bps = 500;
        c.macro_state.nutrient_burst_until = 4;
        c.macro_state.current_epoch = 1;
        let m3 = c.get_appetite_meter();
        assert_eq!(m3.burst_bps, 500);
        assert_eq!(m3.burst_until, 4);
        assert_eq!(m3.burst_epochs_left, 3, "burst_epochs_left = until - current");

        // 5. Expired Nutrient Burst zeros out in appetite meter
        c.macro_state.current_epoch = 4;
        let m4 = c.get_appetite_meter();
        assert_eq!(m4.burst_bps, 0, "Expired burst must reflect 0 bps");
        assert_eq!(m4.burst_epochs_left, 0);
    }

    // =========================================================================
    // QA-129 (TC-SVG-04): zone_changed NEP-297 Event Transitions
    // =========================================================================

    /// QA-129: Verify `zone_changed` events are emitted when transitioning between zones,
    /// and that no duplicate event is emitted if subsequent actions stay in the same zone.
    #[test]
    fn tc_svg_04_zone_changed_events_and_no_duplicate_spurious_emission() {
        let mut c = fresh_contract();

        // 1. Initial incubation sets default last_zone = Normal.
        // Action 1: Extract at low temperature (<600 mK -> HYPO).
        // CrucibleTemp drops to 500 -> transitions NORMAL -> HYPO.
        let fee = 10_000u128;
        let supply = c.macro_state.total_snacc_supply; // 100_000
        // Set volume so after adding fee, temp = 500
        c.macro_state.volume_epoch = 500 * supply / 1000 - fee;
        c.macro_state.last_update_ms = T0 * 1000;

        ctx(ALKEMIS, fee, T0);
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(fee));

        let l1 = get_logs().join("\n");
        assert!(l1.contains(r#""event":"zone_changed""#), "Must emit zone_changed event");
        assert!(l1.contains(r#""from_zone":"NORMAL""#));
        assert!(l1.contains(r#""to_zone":"HYPO""#));
        assert!(l1.contains(r#""crucible_temp":500"#));
        assert_eq!(c.macro_state.last_zone, crate::state::MacroZone::Hypothermia);

        // Action 2: Another extract still in HYPO zone (550 mK).
        // Since from_zone == HYPO and to_zone == HYPO, NO new zone_changed event must be emitted.
        c.macro_state.volume_epoch = 550 * supply / 1000 - fee;
        c.macro_state.last_update_ms = T0 * 1000;

        ctx(ALKEMIS, fee, T0);
        let _ = c.extract_spores(1, "mt:spore-ignis".to_string(), U128(fee));

        let l2 = get_logs();
        // get_logs() drains the logs queue, so l2 contains only logs from Action 2.
        // It must NOT contain any zone_changed event.
        let zone_events_during_action_2 = l2.iter().filter(|log| log.contains(r#""event":"zone_changed""#)).count();
        assert_eq!(
            zone_events_during_action_2, 0,
            "No duplicate zone_changed event should be emitted when remaining in HYPO"
        );

        // Action 3: Transmute pushing temperature into Calcination Fever (800 mK > 610).
        // Role check: add Fermenter role
        let mut v = c.strains.get(&1).unwrap().clone();
        v.role_mask = 0b011; // Extractor + Fermenter
        let _ = c.strains.insert(1, v);
        let _ = c.inventories.insert((1, "mt:spore-ignis".to_string()), 4);

        // Set volume so after diet/fee, temp = 800 mK
        c.macro_state.volume_epoch = 800 * supply / 1000 - 8_000;
        c.macro_state.last_update_ms = T0 * 1000;

        ctx(ALKEMIS, 8_000, T0);
        let _ = c.transmute_spores_to_enzyme(1, 2, U128(10_000));

        let l3 = get_logs();
        let zone_events_during_action_3 = l3.iter().filter(|log| log.contains(r#""event":"zone_changed""#)).count();
        assert_eq!(
            zone_events_during_action_3, 1,
            "A zone_changed event must be emitted for HYPO -> FEVER transition"
        );
        let last_log = l3.iter().find(|log| log.contains(r#""event":"zone_changed""#)).unwrap();
        assert!(last_log.contains(r#""from_zone":"HYPO""#));
        assert!(last_log.contains(r#""to_zone":"FEVER""#));
        assert!(last_log.contains(r#""crucible_temp":800"#));
        assert_eq!(c.macro_state.last_zone, crate::state::MacroZone::Fever);
    }
}

//! Emitter event standar `NEP-297` (`EVENT_JSON:`) — `VC-010`.
//!
//! Format envelope (NEP-297):
//! `EVENT_JSON:{"standard":"snacc","version":"1.0.0","event":"<kind>","data":[<payload>]}`
//! di mana `<kind>` diambil dari field `"kind"` pada payload JSON yang dikirim pemanggil.
//!
//! Event yang dipancarkan (sesuai spec F1–F7): `strain_created`, `extracted`,
//! `transmuted`, `synthesized`, `genome_committed`, `genome_mutated`,
//! `strains_spliced`, `strain_liquidated`; event lanjutan: `strain_divided`,
//! `strain_adopted`, `nutrient_burst_released`, `zone_changed`,
//! `aether_burned_for_dividend`, `dividend_claimed`, `epoch_dividend_closed`.

/// Nama standard event (`NEP-297` field `standard`).
pub const STANDARD: &str = "snacc";
/// Versi standard event (`NEP-297` field `version`).
pub const VERSION: &str = "1.0.0";

/// Emit event `NEP-297` ber-standard `"snacc"` dari payload `{"kind":"<event>", ...}`.
///
/// `kind` diangkat menjadi field `event` di envelope; payload utuh masuk `data[0]`
/// sehingga konsumen (indexer `M5`, parser `SDK-107..108`) menerima seluruh detail.
pub fn emit_event_json(payload_json: &str) {
    let event = extract_kind(payload_json).unwrap_or("unknown");
    near_sdk::env::log_str(&format!(
        r#"EVENT_JSON:{{"standard":"{}","version":"{}","event":"{}","data":[{}]}}"#,
        STANDARD, VERSION, event, payload_json
    ));
}

/// Ambil nilai `"kind":"<event>"` dari payload JSON (parsing ringan, tanpa dependensi).
fn extract_kind(payload_json: &str) -> Option<&str> {
    let key = "\"kind\":\"";
    let start = payload_json.find(key)? + key.len();
    let rest = &payload_json[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vc010_extract_kind_dari_payload() {
        assert_eq!(
            extract_kind(r#"{"kind":"strain_created","strain_id":1}"#),
            Some("strain_created")
        );
        assert_eq!(extract_kind(r#"{"x":1}"#), None);
    }

    #[test]
    fn vc010_envelope_nep297_format() {
        let mut builder = near_sdk::test_utils::VMContextBuilder::new();
        builder.block_timestamp(3_600_000_000_000);
        near_sdk::testing_env!(builder.build());
        emit_event_json(r#"{"kind":"extracted","strain_id":1}"#);
        let logs = near_sdk::test_utils::get_logs();
        let last = logs.last().unwrap();
        assert!(last.starts_with("EVENT_JSON:{"));
        assert!(last.contains(r#""standard":"snacc""#));
        assert!(last.contains(r#""version":"1.0.0""#));
        assert!(last.contains(r#""event":"extracted""#));
        assert!(last.contains(r#""data":[{"kind":"extracted","strain_id":1}]"#));
    }
}

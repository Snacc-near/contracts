//! `NEP-297` standard event emitter (`EVENT_JSON:`) — `VC-010`.
//!
//! Envelope format (NEP-297):
//! `EVENT_JSON:{"standard":"snacc","version":"1.0.0","event":"<kind>","data":[<payload>]}`
//! where `<kind>` is taken from the `"kind"` field of the JSON payload sent by the caller.
//!
//! Emitted events (per spec F1–F7): `strain_created`, `extracted`,
//! `transmuted`, `synthesized`, `genome_committed`, `genome_mutated`,
//! `strains_spliced`, `strain_liquidated`; follow-up events: `strain_divided`,
//! `strain_adopted`, `nutrient_burst_released`, `zone_changed`,
//! `aether_burned_for_dividend`, `dividend_claimed`, `epoch_dividend_closed`.

/// The event standard name (the `NEP-297` `standard` field).
pub const STANDARD: &str = "snacc";
/// The event standard version (the `NEP-297` `version` field).
pub const VERSION: &str = "1.0.0";

/// Emit a `NEP-297` event with standard `"snacc"` from a `{"kind":"<event>", ...}` payload.
///
/// `kind` is lifted into the envelope's `event` field; the whole payload lands in `data[0]`
/// so consumers (the `M5` indexer, the `SDK-107..108` parsers) receive every detail.
pub fn emit_event_json(payload_json: &str) {
    emit_standard_event(STANDARD, extract_kind(payload_json).unwrap_or("unknown"), payload_json);
}

/// Emit a `NEP-297` event with an explicit `standard` — used by external standards
/// (`nep141` for the `ft_*` events per the `N141-101..106` decision).
/// `event` is the event name (the envelope's `event` field); the whole payload lands in `data[0]`.
pub fn emit_standard_event(standard: &str, event: &str, payload_json: &str) {
    near_sdk::env::log_str(&format!(
        r#"EVENT_JSON:{{"standard":"{}","version":"{}","event":"{}","data":[{}]}}"#,
        standard, VERSION, event, payload_json
    ));
}

/// Extract the `"kind":"<event>"` value from a JSON payload (lightweight parsing, no dependencies).
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
    fn vc010_extract_kind_from_payload() {
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

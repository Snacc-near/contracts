//! M2 — Generator `NEP-177` Dynamic Base64 SVG (`F7`, `VC-009`).
//!
//! Kanonik ([[Snacc - Model Data]] §Kamus Angka Resmi): **1.734 byte** batas atas,
//! 8 indikator real-time (heartbeat, `$SNACC`, `$ATP`, slot organel, suhu, zona,
//! koloni/generasi, bounty) + `role_mask` & `generation` (spike `S1.5`).
//! Status `Liquidated` → merah gelap `#ff2255` + `STATUS: LIQUIDATED` (spec `F6`).

/// Base64 standar (padding `=`) — encoder sendiri, nol dependensi tambahan.
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Render SVG vitalitas satu Strain — deterministik, ≤ 1.734 byte.
#[allow(clippy::too_many_arguments)]
pub fn render_strain_svg(
    strain_id: u64,
    role_mask: u8,
    generation: u8,
    snacc_balance: u128,
    atp_balance: u128,
    mitokondria: u8,
    nukleus: u8,
    lisosom: u8,
    heartbeat_sec: u64,
    bounty_snacc: u128,
    temp_mk: u64,
    zone: &str,
    liquidated: bool,
) -> String {
    let (bg, status) = if liquidated {
        ("#ff2255", "STATUS: LIQUIDATED")
    } else {
        ("#0b0f1a", "ALIVE")
    };
    format!(
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320">"#,
            r#"<rect width="320" height="320" fill="{bg}"/>"#,
            r##"<g font-family="monospace" font-size="13" fill="#7df">"##,
            r#"<text x="12" y="30">SNACC #{id} R{role} G{gen}</text>"#,
            r#"<text x="12" y="62">♥ {hb}s | ☠ {bounty}</text>"#,
            r#"<text x="12" y="86">💎 {snacc} | ⚡ {atp} ATP</text>"#,
            r#"<text x="12" y="110">🧬 M{mito} N{nuk} L{liso}</text>"#,
            r#"<text x="12" y="134">🌡 {temp} mK [{zone}]</text>"#,
            r#"<text x="12" y="158">{status}</text>"#,
            "</g></svg>"
        ),
        bg = bg,
        id = strain_id,
        role = role_mask,
        gen = generation,
        hb = heartbeat_sec,
        bounty = bounty_snacc,
        snacc = snacc_balance,
        atp = atp_balance,
        mito = mitokondria,
        nuk = nukleus,
        liso = lisosom,
        temp = temp_mk,
        zone = zone,
        status = status
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_svg() -> String {
        render_strain_svg(1, 0b011, 2, 8_000, 42, 1, 0, 0, 12, 800, 1_190, "NORMAL", false)
    }

    #[test]
    fn vc009_base64_vektor_dikenal() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"abc"), "YWJj");
        assert_eq!(base64_encode(b"abcd"), "YWJjZA==");
    }

    #[test]
    fn vc009_svg_deterministik_dan_batas_ukuran() {
        let a = sample_svg();
        let b = sample_svg();
        assert_eq!(a, b);
        assert!(a.len() <= 1_734, "SVG {} byte > batas kanonik 1.734", a.len());
        assert!(a.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    }

    #[test]
    fn vc009_svg_delapan_indikator() {
        let s = sample_svg();
        for needle in ["SNACC #", "R3", "G2", "♥", "☠", "ATP", "M1 N0 L0", "mK", "NORMAL"] {
            assert!(s.contains(needle), "indikator hilang: {needle}");
        }
    }

    #[test]
    fn vc009_svg_liquidated_merah() {
        let s = render_strain_svg(9, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, "HYPO", true);
        assert!(s.contains("#ff2255"));
        assert!(s.contains("STATUS: LIQUIDATED"));
    }
}

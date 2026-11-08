//! DK-026 — property test `CrucibleTemp` terhadap supply yocto 10^18..10^30:
//! zona harus TERJANGKAU di seluruh rentang skala (aktivitas proporsional
//! V = c×M menghasilkan suhu yang sama — invarian skala).
#[cfg(test)]
#[allow(clippy::module_inception)] // nama modul = nama file qa_*; dipertahankan agar path test tidak berubah
mod qa_dk026_tests {
    use crate::state::MacroState;

    /// MacroState minimal untuk uji rumus murni (field pub).
    fn macro_dengan(volume: u128, supply: u128) -> MacroState {
        MacroState {
            volume_epoch: volume,
            total_snacc_supply: supply,
            ..Default::default()
        }
    }

    /// Property 1 — zona tercapai di SEMUA skala supply 1e18..1e30:
    /// V = 0.61×M → 610 mK (ambang Fever), V = 0.30×M → 300 mK (Hypo),
    /// V = 0 → 0 mK (Hypo). Skala log-spacing 18..=30.
    #[test]
    fn tc_dk026_zona_tercapai_semua_skala_supply() {
        for exp in 18u32..=30 {
            let supply = 10u128.pow(exp);
            // V = c% × M → temp = 1000 × c/100 = 10×c mK.
            for (c, temp_diharapkan) in [(0u128, 0u64), (3, 30), (61, 610), (150, 1500)] {
                let volume = supply * c / 100;
                let m = macro_dengan(volume, supply);
                let temp = m.crucible_temp_at(0);
                assert_eq!(
                    temp, temp_diharapkan,
                    "supply 1e{exp}: V={c}% harus {temp_diharapkan} mK, dapat {temp}"
                );
            }
        }
    }

    /// Property 2 — invarian skala penuh: dua ekonomi berbeda 6 orde magnitude
    /// (dalam rentang uji 1e18..1e30) dengan proporsi aktivitas identik → suhu identik.
    #[test]
    fn tc_dk026_invarian_skala_dua_ekonomi() {
        for exp2 in 18u32..=24 {
            let kecil = macro_dengan(10u128.pow(exp2) * 61 / 100, 10u128.pow(exp2));
            let besar = macro_dengan(10u128.pow(exp2 + 6) * 61 / 100, 10u128.pow(exp2 + 6));
            assert_eq!(
                kecil.crucible_temp_at(0),
                besar.crucible_temp_at(0),
                "ekonomi 1e{exp2} vs 1e{} (V 61%) harus suhu identik",
                exp2 + 6
            );
        }
    }

    /// Property 3 — monotin: suhu tidak turun saat volume naik (skala mana pun).
    #[test]
    fn tc_dk026_monoton_dalam_volume() {
        let supply = 10u128.pow(24);
        let mut prev = 0u64;
        // V = c% × M untuk c naik → temp = 10×c mK, monoton; kasus ekstrem → clamp u64::MAX.
        for c in [0u128, 1, 10, 61, 150, 1_000] {
            let m = macro_dengan(supply * c / 100, supply);
            let t = m.crucible_temp_at(0);
            assert!(t >= prev, "suhu monoton naik: {t} < {prev}");
            prev = t;
        }
        let m = macro_dengan(u128::MAX / 2, supply);
        assert!(m.crucible_temp_at(0) >= prev, "clamp tetap >= suhu sebelumnya");
    }

    /// Property 4 — tanpa wrap u64/u128 di skala ekstrem (V besar, supply kecil).
    #[test]
    fn tc_dk026_tanpa_wrap_skala_ekstrem() {
        let m = macro_dengan(u128::MAX / 2, 10u128.pow(18));
        let t = m.crucible_temp_at(0);
        assert_eq!(t, u64::MAX, "clamped ke u64::MAX, tidak wrap");
    }
}

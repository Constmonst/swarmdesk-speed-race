//! How much less fuel a candidate must burn than the record holder to take its place.

/// A submission must burn at least this much less fuel than the record holder when no baseline
/// taper applies.
pub const MARGIN_PERCENT: u64 = 3;

/// Required win margin in basis points (`100` bps = 1%). Tapers as `holder_fuel` drops
/// significantly below `baseline_fuel`, keeping late-stage optimizations reachable.
pub fn required_margin_bps(holder_fuel: u64, baseline_fuel: Option<u64>) -> u64 {
    let Some(base) = baseline_fuel.filter(|&b| b > 0) else {
        return MARGIN_PERCENT * 100;
    };
    let holder = u128::from(holder_fuel) * 100;
    let base = u128::from(base);
    if holder >= base * 60 {
        300
    } else if holder >= base * 35 {
        150
    } else {
        50
    }
}

/// True when the candidate burns at least `margin_bps` basis points less fuel than the holder.
pub fn wins_bps(candidate_fuel: u64, holder_fuel: u64, margin_bps: u64) -> bool {
    u128::from(candidate_fuel) * 10_000
        <= u128::from(holder_fuel) * u128::from(10_000u64.saturating_sub(margin_bps))
}

/// True when the candidate burns at least `MARGIN_PERCENT` less fuel.
pub fn wins(candidate_fuel: u64, holder_fuel: u64) -> bool {
    wins_bps(candidate_fuel, holder_fuel, MARGIN_PERCENT * 100)
}

pub(crate) fn format_margin_percent(bps: u64) -> String {
    if bps.is_multiple_of(100) {
        format!("{}", bps / 100)
    } else if bps.is_multiple_of(10) {
        format!("{}.{}", bps / 100, (bps % 100) / 10)
    } else {
        format!("{}.{:02}", bps / 100, bps % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_margin_percent, required_margin_bps, wins, wins_bps};

    #[test]
    fn margin_is_exact() {
        assert!(wins(97, 100));
        assert!(!wins(98, 100));
        assert!(!wins(100, 100));
        assert!(wins(0, 100));
    }

    #[test]
    fn tapered_margin_tiers() {
        assert_eq!(required_margin_bps(500, None), 300);
        assert_eq!(required_margin_bps(500, Some(0)), 300);
        assert_eq!(required_margin_bps(600, Some(1_000)), 300);
        assert_eq!(required_margin_bps(599, Some(1_000)), 150);
        assert_eq!(required_margin_bps(350, Some(1_000)), 150);
        assert_eq!(required_margin_bps(349, Some(1_000)), 50);
        assert_eq!(required_margin_bps(0, Some(1_000)), 50);
    }

    #[test]
    fn wins_bps_handles_tapered_margins_and_large_fuel() {
        assert!(wins_bps(985, 1_000, 150));
        assert!(!wins_bps(986, 1_000, 150));
        assert!(wins_bps(995, 1_000, 50));
        assert!(!wins_bps(996, 1_000, 50));
        assert!(wins_bps(u64::MAX - 10_000, u64::MAX, 0));
    }

    #[test]
    fn margin_percent_prints_without_trailing_zeros() {
        assert_eq!(format_margin_percent(300), "3");
        assert_eq!(format_margin_percent(150), "1.5");
        assert_eq!(format_margin_percent(55), "0.55");
    }
}

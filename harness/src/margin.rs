//! How much less fuel a candidate must burn than the record holder to take its place.

/// A submission must burn at least this much less fuel than the record holder when no taper
/// applies.
pub const MARGIN_PERCENT: u64 = 3;

/// Required win margin in basis points (`100` bps = 1%), from the record holder's share of the
/// original reference's fuel in basis points (`10_000` = no record yet). The share is the
/// product of each record's own ratio, each measured on one set of inputs, so it does not move
/// with the random inputs of this run the way raw fuel totals do. Below 60% the margin tapers
/// to 1.5%, below 35% to 0.5%, keeping late-stage optimizations reachable.
pub fn required_margin_bps(holder_share_bps: Option<u64>) -> u64 {
    match holder_share_bps {
        Some(share) if share < 3_500 => 50,
        Some(share) if share < 6_000 => 150,
        _ => MARGIN_PERCENT * 100,
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
        assert_eq!(required_margin_bps(None), 300);
        assert_eq!(required_margin_bps(Some(10_000)), 300);
        assert_eq!(required_margin_bps(Some(6_000)), 300);
        assert_eq!(required_margin_bps(Some(5_999)), 150);
        assert_eq!(required_margin_bps(Some(3_500)), 150);
        assert_eq!(required_margin_bps(Some(3_499)), 50);
        assert_eq!(required_margin_bps(Some(0)), 50);
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

//! Day or night, from Plasma's NightTime schedule (SPEC.md §6).

/// One NightTime cycle in ms since the epoch: (noon, morning start, morning end,
/// evening start, evening end).
pub type Cycle = (i64, i64, i64, i64, i64);

/// Day runs from the middle of the morning transition to the middle of the evening one.
pub fn is_day(now_ms: i64, cycles: &[Cycle]) -> bool {
    cycles.iter().any(|&(_, ms, me, es, ee)| {
        let sunrise = ms + (me - ms) / 2;
        let sunset = es + (ee - es) / 2;
        (sunrise..sunset).contains(&now_ms)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // a real cycle from knighttimed (Istanbul, 2026-11-01)
    const CYCLE: Cycle = (1793440088000, 1793419543000, 1793421251000, 1793458924000, 1793460633000);

    #[test]
    fn noon_is_day() {
        assert!(is_day(CYCLE.0, &[CYCLE]));
    }

    #[test]
    fn before_dawn_and_after_dusk_are_night() {
        assert!(!is_day(CYCLE.1 - 1, &[CYCLE]));
        assert!(!is_day(CYCLE.4 + 1, &[CYCLE]));
        assert!(!is_day(CYCLE.0, &[]));
    }

    #[test]
    fn switches_mid_transition() {
        let mid_morning = CYCLE.1 + (CYCLE.2 - CYCLE.1) / 2;
        assert!(!is_day(mid_morning - 1, &[CYCLE]));
        assert!(is_day(mid_morning, &[CYCLE]));
    }
}

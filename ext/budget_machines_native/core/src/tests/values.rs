//! The validating constructors of the public-field and returned values:
//! everything the core produces is accepted back, nothing else is.

use core::time::Duration;

use super::{SILENT, XorShift64, counts, objective, pair, rule, t};
use crate::{Account, Change, Decision, Ledger, Monitor, OnNoData, Standing, State, Verdict};

fn rebuilt(account: Account) -> Option<Account> {
    Account::new(
        account.period(),
        account.counts(),
        account.base(),
        account.allowance(),
        account.standing(),
        account.consumed_milli(),
    )
}

fn again(decision: Decision) -> Option<Decision> {
    Decision::new(
        decision.state(),
        decision.change(),
        decision.long_milli(),
        decision.short_milli(),
        decision.since(),
    )
}

#[test]
fn allowance_matches_the_wide_formula() {
    let mut rng = XorShift64::new(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let seed = rng.next_u64();
        let per = u32::try_from(seed >> 40).unwrap_or(u32::MAX).max(1);
        let good = u32::try_from(seed & 0xFFFF_FFFF).unwrap_or(0) % per;
        let total = seed.rotate_left(17);
        let wide = u128::from(total) * u128::from(per - good) / u128::from(per);
        assert_eq!(u128::from(objective(good, per).allowance(total)), wide);
    }
}

#[test]
fn every_account_a_ledger_produces_is_accepted_back() {
    for (good, per) in [
        (0, 1),
        (2, 3),
        (99, 100),
        (999, 1000),
        (u32::MAX - 1, u32::MAX),
    ] {
        let mut ledger = Ledger::new(objective(good, per), 1);
        assert_eq!(rebuilt(ledger.account()), Some(ledger.account()));
        for reading in [
            (0, 0),
            (10, 10),
            (9, 10),
            (0, 10),
            (999, 1000),
            (0, u64::MAX),
        ] {
            let replaced = ledger.replace(1, counts(reading.0, reading.1));
            assert_eq!(replaced, Ok(()));
            for planned in [reading.1, reading.1.saturating_add(7), u64::MAX] {
                let account = ledger
                    .account_against(planned)
                    .expect("plan at or above the observed total");
                assert_eq!(rebuilt(account), Some(account));
            }
        }
    }
}

#[test]
fn accounts_no_ledger_could_produce_are_refused() {
    let within = |remaining| Standing::Within { remaining };
    let over = Standing::Exhausted { overspent: 1 };
    let (none, five) = (counts(0, 0), counts(4, 5));
    for (counts, base, allowance, standing, consumed) in [
        // The base below the observed total, an allowance above the base.
        (five, 4, 1, within(0), Some(1000)),
        (five, 5, 6, within(5), Some(166)),
        // A standing that disagrees with the counts and allowance.
        (five, 5, 2, within(0), Some(500)),
        (five, 5, 2, Standing::NoData, Some(500)),
        (five, 5, 2, over, Some(500)),
        (none, 0, 0, within(0), None),
        // A consumption that cannot belong to the base or the standing.
        (five, 5, 1, within(0), None),
        (none, 0, 0, Standing::NoData, Some(0)),
        (counts(5, 5), 5, 1, within(1), Some(3)),
        (five, 5, 1, within(0), Some(1001)),
        (five, 5, 0, over, Some(999)),
    ] {
        let account = Account::new(1, counts, base, allowance, standing, consumed);
        assert_eq!(account, None, "{counts:?} {base} {allowance} {standing:?}");
    }
    assert!(Account::new(1, five, 5, 1, within(0), Some(1000)).is_some());
    assert!(Account::new(9, none, 0, 0, Standing::NoData, None).is_some());
}

#[test]
fn every_decision_and_verdict_an_evaluation_produces_is_accepted_back() {
    for policy in [OnNoData::Hold, OnNoData::Reset, OnNoData::Fire] {
        let mut monitor = Monitor::<3>::new(objective(999, 1000), policy, 10);
        for added in [rule(14_400, 14_400, 2), rule(6000, 3000, 1)] {
            assert!(monitor.add_rule(added).is_ok());
        }
        let readings = [
            [pair((900, 1000), (90, 100)), SILENT],
            [pair((1000, 1000), (100, 100)), pair((0, 5), (0, 5))],
            [SILENT, pair((990, 1000), (99, 100))],
            [pair((999, 1000), (1, 1)), pair((500, 1000), (50, 100))],
        ];
        for (minute, windows) in (0..).zip(readings.iter().cycle().take(12)) {
            let verdict = monitor
                .evaluate(t(minute), windows)
                .expect("in order and shaped");
            for &decision in verdict.decisions() {
                assert_eq!(again(decision), Some(decision));
            }
            let back = Verdict::<3>::new(verdict.decisions(), verdict.worst());
            assert_eq!(back, Some(verdict));
        }
    }
}

#[test]
fn decisions_and_verdicts_no_evaluation_could_produce_are_refused() {
    let at = Some(Duration::from_secs(1));
    let (long, short) = (Some(1), Some(2));
    for (state, change, short, since) in [
        // An edge into the wrong state, or without its instant.
        (State::Quiet, Change::Raised, short, at),
        (State::Firing, Change::Cleared, short, at),
        (State::Firing, Change::Raised, short, None),
        // Both rates measured, yet the state says no data.
        (State::NoData, Change::None, short, at),
    ] {
        assert_eq!(Decision::new(state, change, long, short, since), None);
    }
    let firing = Decision::new(State::Firing, Change::Raised, None, None, at)
        .expect("the fire policy raises without data");
    let quiet = Decision::new(State::Quiet, Change::None, long, short, None)
        .expect("a measured quiet decision");
    assert_eq!((firing.measured(), quiet.measured()), (false, true));
    assert_eq!(Verdict::<1>::new(&[quiet, quiet], None), None);
    for worst in [None, Some(0), Some(2)] {
        assert_eq!(Verdict::<2>::new(&[quiet, firing], worst), None);
    }
    assert!(Verdict::<2>::new(&[quiet, firing], Some(1)).is_some());
    assert!(Verdict::<2>::new(&[], None).is_some());
}

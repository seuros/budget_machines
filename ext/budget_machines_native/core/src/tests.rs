extern crate std;

use core::time::Duration;
use std::vec::Vec;

use matryona_machines::testing::{XorShift64, refuses};

use super::*;

mod model;
mod values;

/// A draw of `rng` in `0..bound`, narrowed to `u32`.
fn below_u32(rng: &mut XorShift64, bound: u32) -> u32 {
    u32::try_from(rng.below(u64::from(bound))).expect("below a u32 bound")
}

/// Test time in whole minutes since the epoch the test picked.
const fn t(minutes: u64) -> Duration {
    Duration::from_secs(minutes * 60)
}

fn objective(good: u32, per: u32) -> Objective {
    Objective::new(good, per).expect("valid objective")
}

fn counts(good: u64, total: u64) -> Counts {
    Counts::new(good, total).expect("valid counts")
}

fn rule(fire: u32, clear: u32, severity: u8) -> Rule {
    Rule::new(fire, clear, severity).expect("valid rule")
}

fn pair(long: (u64, u64), short: (u64, u64)) -> Windows {
    Windows {
        long: Sample::Counts(counts(long.0, long.1)),
        short: Sample::Counts(counts(short.0, short.1)),
    }
}

const SILENT: Windows = Windows {
    long: Sample::NoData,
    short: Sample::NoData,
};

#[test]
fn constructors_reject_impossible_inputs() {
    assert_eq!(Counts::new(2, 1), None);
    assert_eq!(Objective::new(1000, 1000), None);
    assert_eq!(Objective::new(1, 0), None);
    assert_eq!(Objective::new(0, 0), None);
    assert_eq!(Rule::new(1000, 0, 0), None);
    assert_eq!(Rule::new(1000, 1001, 0), None);
    assert!(Rule::new(1000, 1000, 0).is_some());
    assert!(Objective::new(0, 1).is_some());
    assert_eq!(counts(u64::MAX, u64::MAX).checked_add(counts(0, 1)), None);
}

#[test]
fn burn_rate_is_exact_fixed_point() {
    let slo = objective(999, 1000);
    assert_eq!(slo.burn_milli(counts(0, 0)), None);
    assert_eq!(slo.burn_milli(counts(1000, 1000)), Some(0));
    assert_eq!(slo.burn_milli(counts(999, 1000)), Some(1000));
    assert_eq!(slo.burn_milli(counts(9856, 10_000)), Some(14_400));
    assert!(slo.burns_at_least(counts(9856, 10_000), 14_400));
    assert!(!slo.burns_at_least(counts(9857, 10_000), 14_400));
    assert!(!slo.burns_at_least(counts(0, 0), 1));
    // One third of a 1/3 budget is not representable in binary floating point.
    let thirds = objective(2, 3);
    assert_eq!(thirds.burn_milli(counts(2, 3)), Some(1000));
    assert!(thirds.burns_at_least(counts(2, 3), 1000));
    assert!(!thirds.burns_at_least(counts(2, 3), 1001));
}

#[test]
fn u64_and_u32_limits_neither_overflow_nor_wrap() {
    let tight = objective(u32::MAX - 1, u32::MAX);
    let all_bad = counts(0, u64::MAX);
    assert_eq!(tight.allowance(u64::MAX), u64::MAX / u64::from(u32::MAX));
    assert_eq!(tight.burn_milli(all_bad), Some(u64::from(u32::MAX) * 1000));
    assert!(tight.burns_at_least(all_bad, u32::MAX));
    let loose = objective(0, u32::MAX);
    assert_eq!(loose.allowance(u64::MAX), u64::MAX);
    assert_eq!(loose.burn_milli(all_bad), Some(1000));
    assert!(loose.burns_at_least(all_bad, 1000));
    assert!(!loose.burns_at_least(all_bad, 1001));
    assert!(!loose.burns_at_least(counts(1, u64::MAX), u32::MAX));
    // Valid counts top out at per * 1000; only the raw form can saturate.
    assert_eq!(tight.burn_milli_of(u64::MAX, 1), Some(u64::MAX));
}

#[test]
fn ledger_accounts_and_distinguishes_no_data() {
    let mut ledger = Ledger::new(objective(99, 100), 7);
    let empty = ledger.account();
    assert_eq!(empty.standing(), Standing::NoData);
    assert_eq!(empty.consumed_milli(), None);
    assert_eq!(ledger.record(7, counts(995, 1000)), Ok(()));
    let account = ledger.account();
    assert_eq!(account.allowance(), 10);
    assert_eq!(account.standing(), Standing::Within { remaining: 5 });
    assert_eq!(account.consumed_milli(), Some(500));
    assert_eq!(ledger.record(7, counts(0, 5)), Ok(()));
    assert_eq!(
        ledger.account().standing(),
        Standing::Within { remaining: 0 }
    );
    assert_eq!(ledger.record(7, counts(0, 1)), Ok(()));
    assert_eq!(
        ledger.account().standing(),
        Standing::Exhausted { overspent: 1 }
    );
    assert_eq!(ledger.replace(7, counts(1000, 1000)), Ok(()));
    assert_eq!(
        ledger.account().standing(),
        Standing::Within { remaining: 10 }
    );
}

#[test]
fn ledger_plans_against_a_forecast() {
    let mut ledger = Ledger::new(objective(999, 1000), 0);
    assert_eq!(ledger.record(0, counts(9, 10)), Ok(()));
    assert_eq!(
        ledger.account().standing(),
        Standing::Exhausted { overspent: 1 }
    );
    let planned = ledger.account_against(100_000);
    assert_eq!(
        planned.map(|account| (account.base(), account.allowance(), account.standing())),
        Ok((100_000, 100, Standing::Within { remaining: 99 }))
    );
    assert_eq!(
        planned.map(|account| account.consumed_milli()),
        Ok(Some(10))
    );
    assert_eq!(ledger.account_against(9), Err(Refusal::BelowObserved));
    let unseen = Ledger::new(objective(999, 1000), 0).account_against(1000);
    assert_eq!(
        unseen.map(|account| (account.allowance(), account.standing())),
        Ok((1, Standing::NoData))
    );
}

#[test]
fn ledger_refusals_leave_it_untouched() {
    let mut ledger = Ledger::new(objective(9, 10), 5);
    assert_eq!(ledger.record(5, counts(u64::MAX - 1, u64::MAX - 1)), Ok(()));
    refuses(&mut ledger, Refusal::StalePeriod, |l| {
        l.record(4, counts(1, 1))
    });
    refuses(&mut ledger, Refusal::StalePeriod, |l| {
        l.record(6, counts(1, 1))
    });
    refuses(&mut ledger, Refusal::StalePeriod, |l| {
        l.replace(4, counts(1, 1))
    });
    refuses(&mut ledger, Refusal::Overflow, |l| {
        l.record(5, counts(2, 2))
    });
    refuses(&mut ledger, Refusal::Overflow, |l| {
        l.record(5, counts(0, 2))
    });
    refuses(&mut ledger, Refusal::StalePeriod, |l| l.roll(5));
    refuses(&mut ledger, Refusal::StalePeriod, |l| l.roll(4));
    assert_eq!(ledger.record(5, counts(1, 1)), Ok(()));
    let closed = ledger.roll(u64::MAX);
    assert_eq!(
        closed.map(|account| (account.period(), account.counts())),
        Ok((5, counts(u64::MAX, u64::MAX)))
    );
    assert_eq!(ledger.period(), u64::MAX);
    assert_eq!(ledger.counts(), Counts::default());
    assert_eq!(ledger.account().standing(), Standing::NoData);
    assert_eq!(ledger.record(5, counts(1, 1)), Err(Refusal::StalePeriod));
    assert_eq!(ledger.roll(u64::MAX), Err(Refusal::StalePeriod));
}

#[test]
fn both_windows_must_burn_and_the_short_one_clears() {
    let mut monitor = Monitor::<2>::new(objective(999, 1000), OnNoData::Hold, 0);
    assert_eq!(monitor.add_rule(rule(14_400, 14_400, 2)), Ok(0));
    assert_eq!(monitor.add_rule(rule(6000, 6000, 1)), Ok(1));
    let hot = (9800, 10_000);
    let calm = (10_000, 10_000);
    // Long window alone: a spike that already ended.
    let verdict = monitor.evaluate(t(1), &[pair(hot, calm), pair(hot, calm)]);
    assert_eq!(verdict.map(|v| v.worst()), Ok(None));
    assert_eq!(monitor.state(0), Some(State::Quiet));
    // Short window alone: a blip not yet significant.
    assert_eq!(
        monitor
            .evaluate(t(2), &[pair(calm, hot), pair(calm, hot)])
            .map(|v| v.worst()),
        Ok(None)
    );
    let verdict = monitor
        .evaluate(t(3), &[pair(hot, hot), pair(calm, hot)])
        .expect("accepted");
    assert_eq!(verdict.worst(), Some(0));
    let [page, ticket] = verdict.decisions() else {
        panic!("two rules");
    };
    assert_eq!(
        (page.state(), page.change(), page.measured(), page.since()),
        (State::Firing, Change::Raised, true, Some(t(3)))
    );
    assert_eq!(
        (page.long_milli(), page.short_milli()),
        (Some(20_000), Some(20_000))
    );
    assert_eq!(
        (ticket.state(), ticket.change(), ticket.since()),
        (State::Quiet, Change::None, Some(t(1)))
    );
    let verdict = monitor
        .evaluate(t(4), &[pair(hot, calm), pair(hot, hot)])
        .expect("accepted");
    assert_eq!(verdict.worst(), Some(1));
    assert_eq!(
        verdict
            .decisions()
            .iter()
            .map(Decision::change)
            .collect::<Vec<_>>(),
        [Change::Cleared, Change::Raised]
    );
}

#[test]
fn hysteresis_holds_until_below_the_clear_threshold() {
    let mut monitor = Monitor::<1>::new(objective(99, 100), OnNoData::Hold, 0);
    assert_eq!(monitor.add_rule(rule(2000, 1000, 0)), Ok(0));
    let step = |monitor: &mut Monitor<1>, now, bad: u64| {
        let window = (100 - bad, 100);
        monitor
            .evaluate(t(now), &[pair(window, window)])
            .map(|verdict| verdict.decisions().first().map(|d| (d.state(), d.change())))
    };
    assert_eq!(
        step(&mut monitor, 0, 1),
        Ok(Some((State::Quiet, Change::None)))
    );
    assert_eq!(
        step(&mut monitor, 1, 2),
        Ok(Some((State::Firing, Change::Raised)))
    );
    assert_eq!(
        step(&mut monitor, 2, 1),
        Ok(Some((State::Firing, Change::None)))
    );
    assert_eq!(
        step(&mut monitor, 3, 0),
        Ok(Some((State::Quiet, Change::Cleared)))
    );
    assert_eq!(
        step(&mut monitor, 4, 1),
        Ok(Some((State::Quiet, Change::None)))
    );
}

#[test]
fn no_data_follows_the_configured_policy() {
    let hot = pair((0, 10), (0, 10));
    let thin = pair((0, 10), (0, 4));
    for (policy, from_start, from_firing) in [
        (OnNoData::Hold, State::NoData, State::Firing),
        (OnNoData::Reset, State::NoData, State::NoData),
        (OnNoData::Fire, State::Firing, State::Firing),
    ] {
        let mut monitor = Monitor::<1>::new(objective(9, 10), policy, 5);
        assert_eq!(monitor.add_rule(rule(1000, 1000, 0)), Ok(0));
        let verdict = monitor.evaluate(t(0), &[SILENT]).expect("accepted");
        let [first] = verdict.decisions() else {
            panic!("one rule");
        };
        assert_eq!((first.state(), first.measured()), (from_start, false));
        assert_eq!((first.long_milli(), first.short_milli()), (None, None));
        assert_eq!(
            monitor.evaluate(t(1), &[hot]).map(|v| v.worst()),
            Ok(Some(0))
        );
        // Below the minimum total is no data even though counts were supplied.
        let verdict = monitor.evaluate(t(2), &[thin]).expect("accepted");
        let [after] = verdict.decisions() else {
            panic!("one rule");
        };
        assert_eq!((after.state(), after.measured()), (from_firing, false));
        assert_eq!(
            (after.long_milli(), after.short_milli()),
            (Some(10_000), None)
        );
        let cleared = from_firing != State::Firing;
        assert_eq!(after.change() == Change::Cleared, cleared);
    }
}

#[test]
fn monitor_refusals_leave_it_untouched() {
    let mut monitor = Monitor::<1>::new(objective(9, 10), OnNoData::Reset, 0);
    assert!(monitor.is_empty());
    assert_eq!(
        monitor.evaluate(t(0), &[]).map(|v| v.decisions().len()),
        Ok(0)
    );
    assert_eq!(monitor.add_rule(rule(1000, 1000, 0)), Ok(0));
    assert_eq!(monitor.add_rule(rule(1000, 1000, 0)), Err(Refusal::Full));
    assert_eq!(monitor.len(), 1);
    assert_eq!((monitor.rule(1), monitor.state(1)), (None, None));
    let hot = pair((0, 10), (0, 10));
    assert_eq!(
        monitor.evaluate(t(10), &[hot]).map(|v| v.worst()),
        Ok(Some(0))
    );
    refuses(&mut monitor, Refusal::TimeRegression, |m| {
        m.evaluate(t(9), &[SILENT])
    });
    refuses(&mut monitor, Refusal::Shape, |m| m.evaluate(t(11), &[]));
    refuses(&mut monitor, Refusal::Shape, |m| {
        m.evaluate(t(11), &[hot, hot])
    });
    assert_eq!(monitor.last(), Some(t(10)));
    // The same instant may be evaluated again; the last one is a valid instant.
    assert_eq!(
        monitor.evaluate(t(10), &[hot]).map(|v| v.worst()),
        Ok(Some(0))
    );
    assert_eq!(
        monitor
            .evaluate(Duration::MAX, &[SILENT])
            .map(|v| v.worst()),
        Ok(None)
    );
    assert_eq!(monitor.state(0), Some(State::NoData));
    assert_eq!(monitor.last(), Some(Duration::MAX));
    assert_eq!(
        Monitor::<0>::new(objective(9, 10), OnNoData::Hold, 0).add_rule(rule(1, 1, 0)),
        Err(Refusal::Full)
    );
}

#[test]
fn worst_prefers_severity_then_order() {
    let mut monitor = Monitor::<3>::new(objective(9, 10), OnNoData::Hold, 0);
    for severity in [1, 5, 5] {
        assert!(monitor.add_rule(rule(1000, 1000, severity)).is_ok());
    }
    let hot = pair((0, 10), (0, 10));
    let calm = pair((10, 10), (10, 10));
    assert_eq!(
        monitor.evaluate(t(0), &[hot, calm, hot]).map(|v| v.worst()),
        Ok(Some(2))
    );
    assert_eq!(
        monitor.evaluate(t(0), &[hot, hot, hot]).map(|v| v.worst()),
        Ok(Some(1))
    );
    assert_eq!(
        monitor
            .evaluate(t(0), &[hot, calm, calm])
            .map(|v| v.worst()),
        Ok(Some(0))
    );
    assert_eq!(monitor.worst(), Some(0));
}

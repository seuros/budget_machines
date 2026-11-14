//! Randomized runs against independent naive models.

use core::time::Duration;

use super::{Rng, counts, objective, rule, std::vec::Vec};
use crate::{Change, Ledger, Monitor, OnNoData, Refusal, Sample, Standing, State, Windows};

/// Burn rate by long division, sharing nothing with the division-free test.
fn naive_milli(good: u32, per: u32, sample: Option<(u64, u64)>, floor: u64) -> Option<u128> {
    let (ok, total) = sample?;
    if total == 0 || total < floor {
        return None;
    }
    let ratio_spent = u128::from(total - ok) * u128::from(per) * 1000;
    Some(ratio_spent / (u128::from(total) * u128::from(per - good)))
}

fn naive_allowance(good: u32, per: u32, total: u64) -> u64 {
    let budget = u128::from(total) * u128::from(per - good);
    let mut whole = 0_u64;
    while u128::from(whole + 1) * u128::from(per) <= budget {
        whole += 1;
    }
    whole
}

/// Replays the whole history from scratch to find one rule's state.
fn naive_state(
    history: &[(Option<u128>, Option<u128>)],
    fire: u32,
    clear: u32,
    policy: OnNoData,
) -> State {
    let mut state = State::NoData;
    for reading in history {
        state = match *reading {
            (Some(long), Some(short)) => {
                let bar = u128::from(if state == State::Firing { clear } else { fire });
                if long >= bar && short >= bar {
                    State::Firing
                } else {
                    State::Quiet
                }
            }
            _ => match policy {
                OnNoData::Hold => state,
                OnNoData::Reset => State::NoData,
                OnNoData::Fire => State::Firing,
            },
        };
    }
    state
}

fn random_sample(rng: &mut Rng) -> (Sample, Option<(u64, u64)>) {
    if rng.below(6) == 0 {
        return (Sample::NoData, None);
    }
    let total = rng.below(40);
    // Skew towards healthy windows so alerts both raise and clear.
    let bad = if rng.below(2) == 0 {
        0
    } else {
        rng.below(total + 1)
    };
    (
        Sample::Counts(counts(total - bad, total)),
        Some((total - bad, total)),
    )
}

#[test]
fn monitor_matches_a_naive_replay_model() {
    const RULES: usize = 3;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut raised = 0_u32;
    let mut cleared = 0_u32;
    for round in 0..300 {
        let per = rng.below_u32(50) + 2;
        let good = rng.below_u32(per);
        let policy = [OnNoData::Hold, OnNoData::Reset, OnNoData::Fire][round % 3];
        let floor = rng.below(8);
        let mut monitor = Monitor::<RULES>::new(objective(good, per), policy, floor);
        let mut rules = Vec::new();
        for _ in 0..RULES {
            let fire = rng.below_u32(6000) + 1;
            let clear = rng.below_u32(fire) + 1;
            assert!(monitor.add_rule(rule(fire, clear, 0)).is_ok());
            rules.push((fire, clear));
        }
        let mut histories = [const { Vec::new() }; RULES];
        // The instant each rule last changed state, kept beside the monitor.
        let mut since = [None; RULES];
        let mut now = Duration::ZERO;
        assert_eq!(monitor.last(), None);
        for _ in 0..40 {
            // Time stands still or jumps ahead by an uneven number of nanoseconds.
            now += Duration::from_nanos(rng.below(3) * rng.below(2_500_000_000));
            let mut windows = Vec::new();
            for history in &mut histories {
                let (long, long_raw) = random_sample(&mut rng);
                let (short, short_raw) = random_sample(&mut rng);
                windows.push(Windows { long, short });
                history.push((
                    naive_milli(good, per, long_raw, floor),
                    naive_milli(good, per, short_raw, floor),
                ));
            }
            let before: Vec<_> = (0..RULES).map(|i| monitor.state(i)).collect();
            let verdict = monitor.evaluate(now, &windows).expect("accepted");
            assert_eq!(monitor.last(), Some(now));
            for (index, decision) in verdict.decisions().iter().enumerate() {
                let (fire, clear) = rules[index];
                let history = &histories[index];
                let expected = naive_state(history, fire, clear, policy);
                assert_eq!(decision.state(), expected, "round {round} now {now:?}");
                if before[index] != Some(expected) {
                    since[index] = Some(now);
                }
                assert_eq!(decision.since(), since[index]);
                assert_eq!(monitor.state(index), Some(expected));
                let (long, short) = history[history.len() - 1];
                assert_eq!(decision.long_milli().map(u128::from), long);
                assert_eq!(decision.short_milli().map(u128::from), short);
                assert_eq!(decision.measured(), long.is_some() && short.is_some());
                let was = before[index] == Some(State::Firing);
                let change = match (was, expected == State::Firing) {
                    (false, true) => Change::Raised,
                    (true, false) => Change::Cleared,
                    (false, false) | (true, true) => Change::None,
                };
                assert_eq!(decision.change(), change);
                raised += u32::from(change == Change::Raised);
                cleared += u32::from(change == Change::Cleared);
            }
        }
    }
    assert!(
        raised > 100 && cleared > 100,
        "{raised} raised, {cleared} cleared"
    );
}

#[test]
fn ledger_matches_a_naive_model() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut seen = [0_u32; 3];
    for _ in 0..400 {
        let per = rng.below_u32(60) + 2;
        let good = rng.below_u32(per);
        let mut period = rng.below(5);
        let mut ledger = Ledger::new(objective(good, per), period);
        let (mut ok, mut total) = (0_u64, 0_u64);
        for _ in 0..30 {
            let size = rng.below(30);
            let bad = if rng.below(3) == 0 {
                rng.below(size + 1)
            } else {
                0
            };
            let window = counts(size - bad, size);
            match rng.below(10) {
                0 => {
                    let next = period + rng.below(3);
                    assert_eq!(ledger.roll(next).is_ok(), next > period);
                    if next > period {
                        (period, ok, total) = (next, 0, 0);
                    }
                }
                1 => {
                    assert_eq!(ledger.replace(period, window), Ok(()));
                    (ok, total) = (window.good(), window.total());
                }
                2 => assert_eq!(ledger.record(period + 1, window), Err(Refusal::StalePeriod)),
                _ => {
                    assert_eq!(ledger.record(period, window), Ok(()));
                    (ok, total) = (ok + window.good(), total + window.total());
                }
            }
            let account = ledger.account();
            let allowance = naive_allowance(good, per, total);
            let bad = total - ok;
            let standing = match (total, allowance.checked_sub(bad)) {
                (0, _) => Standing::NoData,
                (_, Some(remaining)) => Standing::Within { remaining },
                (_, None) => Standing::Exhausted {
                    overspent: bad - allowance,
                },
            };
            assert_eq!(
                (account.period(), account.counts()),
                (period, counts(ok, total))
            );
            assert_eq!(
                (account.allowance(), account.standing()),
                (allowance, standing)
            );
            assert_eq!(
                account.consumed_milli().map(u128::from),
                naive_milli(good, per, Some((ok, total)), 0)
            );
            seen[match standing {
                Standing::NoData => 0,
                Standing::Within { .. } => 1,
                Standing::Exhausted { .. } => 2,
            }] += 1;
        }
    }
    assert!(seen.iter().all(|&count| count > 100), "{seen:?}");
}

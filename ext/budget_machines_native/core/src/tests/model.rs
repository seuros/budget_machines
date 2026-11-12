//! Randomized runs against independent naive models.

use super::{counts, objective, rule, std::vec::Vec};
use crate::{Change, Ledger, Monitor, OnNoData, Refused, Sample, Standing, State, Windows};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

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
        let per = u32::try_from(rng.below(50) + 2).unwrap_or(2);
        let good = u32::try_from(rng.below(u64::from(per))).unwrap_or(0);
        let policy = [OnNoData::Hold, OnNoData::Reset, OnNoData::Fire][round % 3];
        let floor = rng.below(8);
        let mut monitor = Monitor::<RULES>::new(objective(good, per), policy, floor);
        let mut rules = Vec::new();
        for _ in 0..RULES {
            let fire = u32::try_from(rng.below(6000) + 1).unwrap_or(1);
            let clear = u32::try_from(rng.below(u64::from(fire)) + 1).unwrap_or(1);
            assert!(monitor.add_rule(rule(fire, clear, 0)).is_ok());
            rules.push((fire, clear));
        }
        let mut histories = [const { Vec::new() }; RULES];
        for now in 0..40 {
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
            let Ok(verdict) = monitor.evaluate(now, &windows) else {
                panic!("accepted");
            };
            for (index, decision) in verdict.decisions().iter().enumerate() {
                let (fire, clear) = rules[index];
                let history = &histories[index];
                let expected = naive_state(history, fire, clear, policy);
                assert_eq!(decision.state, expected, "round {round} now {now}");
                assert_eq!(monitor.state(index), Some(expected));
                let (long, short) = history[history.len() - 1];
                assert_eq!(decision.long_milli.map(u128::from), long);
                assert_eq!(decision.short_milli.map(u128::from), short);
                assert_eq!(decision.measured, long.is_some() && short.is_some());
                let was = before[index] == Some(State::Firing);
                let change = match (was, expected == State::Firing) {
                    (false, true) => Change::Raised,
                    (true, false) => Change::Cleared,
                    _ => Change::None,
                };
                assert_eq!(decision.change, change);
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
        let per = u32::try_from(rng.below(60) + 2).unwrap_or(2);
        let good = u32::try_from(rng.below(u64::from(per))).unwrap_or(0);
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
                2 => assert_eq!(ledger.record(period + 1, window), Err(Refused::StalePeriod)),
                _ => {
                    assert_eq!(ledger.record(period, window), Ok(()));
                    (ok, total) = (ok + window.good(), total + window.total());
                }
            }
            let account = ledger.account();
            let allowance = naive_allowance(good, per, total);
            let bad = total - ok;
            let standing = if total == 0 {
                Standing::NoData
            } else if bad > allowance {
                Standing::Exhausted {
                    overspent: bad - allowance,
                }
            } else {
                Standing::Within {
                    remaining: allowance - bad,
                }
            };
            assert_eq!(
                (account.period, account.counts),
                (period, counts(ok, total))
            );
            assert_eq!((account.allowance, account.standing), (allowance, standing));
            assert_eq!(
                account.consumed_milli.map(u128::from),
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

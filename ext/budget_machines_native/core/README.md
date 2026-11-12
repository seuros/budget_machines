# budget-machines

> **AMS Beyer**, NCC-4225, *Beyer*-class reliability tender.

Engine-room flight manual: count what the objective tolerates, and say when it
is being spent too fast.

Allocation-free `no_std` SLO error-budget accounting and multi-window burn-rate
alerting. All arithmetic is integer: objectives are exact ratios, burn rates
are thousandths (`MILLI`), and every threshold comparison is a division-free
128-bit cross-multiplication, so two machines never disagree about an alert.

The caller supplies everything: exact good/total counts for each window, the
instant of each evaluation, and the number of the compliance period. The crate
has no clock, no storage and no query engine, and it never rolls a period over
on its own.

- `Objective` is `good` of every `per` events. A 100% objective is refused: it
  has no budget to account.
- `Ledger` accumulates one compliance period. `record` adds a window, `replace`
  takes an authoritative reading, `roll` closes the period. A stale period or
  an overflowing sum is refused and changes nothing. `account` reports the
  whole bad events tolerated, what is left or overspent, and the consumed
  fraction; `account_against` does it against a planned period total.
- `Monitor<R>` holds up to `R` rules. A `Rule` fires when the long and the
  short window both burn at its threshold or more, and clears when either
  falls below its clear threshold. `evaluate` returns one `Decision` per rule
  and the worst firing rule; a backwards instant or the wrong number of window
  pairs is refused and changes nothing.

No data is a state, not a zero. An empty period stands at `Standing::NoData`. A
window that is `Sample::NoData`, empty, or below the monitor's minimum total is
not measured, and the monitor's `OnNoData` policy (`Hold`, `Reset`, `Fire`)
decides the rule's state; `Decision::measured` says which happened.

```rust
use budget_machines::{Counts, Ledger, Monitor, Objective, OnNoData, Rule, Sample, Standing, Windows};

let slo = Objective::new(999, 1000).expect("below 100%");

let mut ledger = Ledger::new(slo, 0);
assert_eq!(ledger.account().standing, Standing::NoData);
ledger.record(0, Counts::new(99_950, 100_000).expect("good <= total")).expect("current period");
assert_eq!(ledger.account().standing, Standing::Within { remaining: 50 });

let mut monitor = Monitor::<4>::new(slo, OnNoData::Hold, 0);
let page = monitor.add_rule(Rule::new(14_400, 14_400, 2).expect("thresholds")).expect("room");
let window = |good, total| Sample::Counts(Counts::new(good, total).expect("good <= total"));
let burning = Windows { long: window(98_000, 100_000), short: window(9_700, 10_000) };
let verdict = monitor.evaluate(1, &[burning]).expect("time moved forward");
assert_eq!(verdict.worst(), Some(page));
```

This is a policy core. It does not collect metrics, page anyone, or decide what
a good event is. Shedding load or opening a breaker because a budget is gone is
a coordinator's decision, made outside this crate.

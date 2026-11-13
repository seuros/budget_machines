# BudgetMachines

> 99.9% is not a compliment. It is a spending limit. 📉

Error-budget arithmetic for Ruby. You tell it your objective and hand it good and total counts. A ledger tells you how many failures the period can still afford, or how far past the limit you are. A monitor takes a long window and a short window per rule and tells you whether the budget is burning fast enough to wake someone, and when it has stopped. All of it in integers, so the answer is the same on every machine, and "we have no data" is never quietly rounded to "we are fine".

## Quick Start

```bash
gem 'budget_machines'
```

```ruby
BM = BudgetMachines

slo = BM::Objective.new(999, 1000)             # 99.9% of requests succeed

ledger = BM::Ledger.new(slo, 0)                # accounted per 30-day period, this is number 0
ledger.record(0, BM::Counts.new(99_950, 100_000))   # good, total for the window just closed
ledger.account.standing                    # => Standing::Within(remaining: 50), 50 failures left

monitor = BM::Monitor.new(slo, :hold, 0)       # objective, no-data policy, minimum events per window
monitor.add_rule(BM::Rule.new(14_400, 14_400, 2))   # page: burning at 14.4x, severity 2
monitor.add_rule(BM::Rule.new(6_000, 6_000, 1))     # ticket: burning at 6x, severity 1

now = Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)
read = ->(good, total) { BM::Sample::Counts.new(BM::Counts.new(good, total)) }
verdict = monitor.evaluate(now, [
  BM::Windows.new(read.(98_000, 100_000), read.(9_700, 10_000)),     # rule 0: long 1h, short 5m
  BM::Windows.new(read.(598_000, 600_000), read.(49_800, 50_000))    # rule 1: long 6h, short 30m
])
verdict.decisions.first.change             # => :raised
verdict.worst                              # => 0, go page someone
```

Two objects and a handful of frozen values. `BM` is only an alias to keep the lines short; don't `include BudgetMachines` at the top level, where a bare `Monitor` is Ruby's own. Every value has the name and the fields of the Rust type it mirrors: `Objective`, `Counts`, `Rule`, `Sample`, `Windows`, `Account`, `Standing`, `Decision`, `Verdict`. Every number is an Integer, and the instant you pass to `evaluate` is Integer nanoseconds of monotonic time. Burn rates and thresholds are in thousandths: `14_400` is 14.4 times the sustainable rate.

## A Message to the Resistance

So you have an SLO. Somebody put "99.9%" on a slide, everybody nodded, and then you had to turn it into an alert. You wrote `error_rate > 0.001 for 5m`. It paged you at 3 a.m. for forty failed requests on a night with no traffic. So you raised the threshold. Then a slow leak ate the entire month's budget over nine days at a rate that never crossed it, and you learned about it from a customer.

So you read the chapter. Burn rates, two windows per alert, a fast one and a slow one. You wrote it as a query. The query divides one float by another and compares the result to `14.4`, and on the day traffic drops to zero it divides zero by zero, gets `NaN`, compares `NaN > 14.4`, gets false, and resolves the page for an outage that is so complete nothing is being counted any more.

The piece you keep rebuilding is small. How many failures does this objective allow in this many events? How fast is this window spending them? Are both windows over the line, and is it still over the line now? And what do you say when a window has nothing in it? That arithmetic is this gem.

## Chapter 1: Stardate 2026.316, the AMS Beyer

> **AMS Beyer**, NCC-4225, *Beyer*-class reliability tender.
> A fleet that never spends its budget is flying too slowly. A fleet that never counts it is already out.

The tender doesn't repair anything. It keeps the books. Every ship in the fleet has an agreed number of bad jumps per thousand, and the tender knows how many each one has left this cycle and how quickly it is going through them. When a ship stops reporting, the tender does not write down zero failures. It writes down "no report", which is a different column.

On a bridge somewhere, a cadet volunteers:

```ruby
def alert?(errors, requests)
  errors / requests.to_f > 0.001
end
```

"Run it with no requests," says the tender's purser. "Then tell me whether the ship is healthy or missing."

## The Flight Plan

Each rule in a monitor is in one of three states:

```mermaid
stateDiagram-v2
    [*] --> no_data
    no_data --> quiet: both windows measured, not both burning
    no_data --> firing: both windows at or over the fire threshold
    quiet --> firing: both windows at or over the fire threshold
    firing --> quiet: either window under the clear threshold
    firing --> firing: a window is missing, policy is hold or fire
    quiet --> no_data: a window is missing, policy is reset
    firing --> no_data: a window is missing, policy is reset
    quiet --> firing: a window is missing, policy is fire
    no_data --> firing: a window is missing, policy is fire
```

And the afternoon the payment API started failing one request in fifty:

```mermaid
sequenceDiagram
    participant Q as Your metrics query
    participant M as Monitor
    participant L as Ledger
    participant P as Your pager

    Q->>M: evaluate(t1, 1h calm, 5m burning 20x)
    M-->>Q: quiet. One window is not enough.
    Q->>M: evaluate(t2, 1h burning 20x, 5m burning 30x)
    M-->>Q: firing, change raised
    Q->>P: page
    Q->>L: record(period, counts)
    L-->>Q: exhausted, 1,900 over
    Note over Q: fix deployed
    Q->>M: evaluate(t3, 1h burning 18x, 5m calm)
    M-->>Q: quiet, change cleared
    Q->>P: resolve
    Note over M: the 1h window is still hot. The 5m one says it's over.
```

## How the Budget Works

### The objective

An `Objective` is two Integers: `good` of every `per` events. `Counts` is a window: `good` of `total`.

```ruby
slo = BM::Objective.new(999, 1000)
slo.tolerated                                              # => 1 failure per 1,000
slo.allowance(100_000)                                     # => 100 failures allowed in 100,000
slo.burn_milli(BM::Counts.new(9_856, 10_000))              # => 14400, i.e. 14.4x
slo.burn_milli_of(144, 10_000)                             # => 14400, from bad and total
slo.burn_milli(BM::Counts.new(0, 0))                       # => nil, no events is not a rate
slo.burns_at_least(BM::Counts.new(9_856, 10_000), 14_400)  # => true, exact, no division

BM::Counts.new(995, 1000).bad                              # => 5
```

A burn rate of `1000` spends the budget exactly as fast as the objective allows. `999, 1000` and `9_990, 10_000` are the same objective. `Objective.new(1000, 1000)` raises `RangeError`: a 100% target has no budget to account. So does `Counts.new(2, 1)`: more good events than events.

### Record

```ruby
ledger = BM::Ledger.new(BM::Objective.new(99, 100), 7)   # 99%, period 7

ledger.record(7, BM::Counts.new(995, 1000))   # => :ok
ledger.record(6, BM::Counts.new(10, 10))      # => :stale_period, nothing added
ledger.counts                                 # => Counts(good: 995, total: 1000)
```

`record` adds a window's counts to the current period. The period is a number you choose and advance. Counts for any other period are refused. A sum that would not fit in 64 bits is refused with `:overflow`.

When your source gives you the running total instead of a delta, use `replace`:

```ruby
ledger.replace(7, BM::Counts.new(995, 1000))  # => :ok, the period now holds exactly this
```

`record` answers `:ok`, or refuses with `:stale_period` or `:overflow`. `replace` answers `:ok`, or refuses with `:stale_period`.

### Account

```ruby
account = ledger.account
account.counts           # => Counts(good: 995, total: 1000)
account.allowance        # => 10   failures 1,000 events may contain at 99%
account.standing         # => Standing::Within(remaining: 5)
account.consumed_milli   # => 500  half the budget

case account.standing
in BM::Standing::Within[remaining:] then "#{remaining} failures left"
in BM::Standing::Exhausted[overspent:] then "#{overspent} over"
in :no_data then 'nothing observed'
end

BM::Ledger.new(BM::Objective.new(99, 100), 7).account.standing   # => :no_data
```

An `Account` has exactly the fields of the core's: `period`, `counts`, `base`, `allowance`, `standing`, `consumed_milli`. `standing` is one of three values, one per variant of the core's `Standing`: `Standing::Within` carries `remaining`, `Standing::Exhausted` carries `overspent`, and the unit variant `NoData` is the symbol `:no_data`, because an empty period has nothing to report. `within?`, `exhausted?` and `no_data?` ask without matching. An empty period has a `nil` `consumed_milli`.

The allowance grows with traffic, so one failure in the first ten requests of a period is exhausted. If you'd rather budget against the traffic you expect:

```ruby
ledger = BM::Ledger.new(BM::Objective.new(999, 1000), 0)
ledger.record(0, BM::Counts.new(9, 10))
ledger.account.standing                    # => Standing::Exhausted(overspent: 1)
ledger.account_against(100_000).standing   # => Standing::Within(remaining: 99)
ledger.account_against(5)                  # => :below_observed
```

`account` always answers an `Account`. `account_against` answers an `Account`, or refuses with `:below_observed`.

### Roll

```ruby
closed = ledger.roll(1)   # => the Account period 0 ended with
ledger.account.standing   # => :no_data
ledger.roll(1)            # => :stale_period
```

`roll` closes the period, hands you its final account, and starts the next one empty. The new period must be greater than the current one. Nothing rolls on its own. `roll` answers the closed `Account`, or refuses with `:stale_period`.

### Rules

```ruby
monitor = BM::Monitor.new(BM::Objective.new(999, 1000), :hold, 0)
monitor.add_rule(BM::Rule.new(14_400, 14_400, 2))   # => 0
monitor.add_rule(BM::Rule.new(6_000, 3_000, 1))     # => 1
monitor.rule(1)    # => Rule(fire_milli: 6000, clear_milli: 3000, severity: 1)
monitor.state(1)   # => :no_data, never measured
monitor.size       # => 2
```

`Monitor.new` takes the objective, the no-data policy and the minimum events a window needs to count as measured. All three are required, as they are in the core: there is no default policy to forget you chose.

A `Rule` is a fire threshold, a clear threshold and a severity. It fires when the long window and the short window both burn at the fire threshold or more. Once firing, it stays firing until either window drops below the clear threshold. Give both the same value for the textbook rule, or a lower clear threshold if you'd like an alert that doesn't flap around the line. A monitor holds `BudgetMachines::CAPACITY` rules, which is 8. `add_rule` answers the new rule's index, or refuses with `:full`.

### Evaluate

```ruby
read = ->(good, total) { BM::Sample::Counts.new(BM::Counts.new(good, total)) }
hot  = read.(9_800, 10_000)    # burning at 20x
calm = read.(10_000, 10_000)
windows = ->(long, short) { BM::Windows.new(long, short) }

minute = 60_000_000_000   # instants are Integer nanoseconds
t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)

monitor.evaluate(t0, [windows.(hot, calm), windows.(hot, calm)]).decisions.map(&:state)
# => [:quiet, :quiet]

verdict = monitor.evaluate(t0 + minute, [windows.(hot, hot), windows.(hot, hot)])
page, ticket = verdict.decisions
page.state                     # => :firing
page.change                    # => :raised
page.long_milli                # => 20000
page.since == t0 + minute      # => true, the instant it started firing
verdict.worst                  # => 0, the firing rule with the highest severity
monitor.last == t0 + minute    # => true

slower = read.(9_950, 10_000)
monitor.evaluate(t0 + (2 * minute), [windows.(hot, calm), windows.(hot, slower)]).decisions.map(&:change)
# => [:cleared, :none]   rule 1 is at 5x: under its fire line, over its clear line
```

You pass the instant and one `Windows` per rule, in rule order. A `Windows` is a `long` and a `short` reading, and a reading is the core's `Sample`: a `Sample::Counts` wrapping a `Counts`, or `:no_data`. The instant is Integer nanoseconds from a monotonic clock, `Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)`; `since` and `last` come back in the same unit. The monitor never sees a window length: you choose the windows and bring their counts. You get a `Verdict`: one `Decision` per rule in `decisions`, and `worst`, the index of the firing rule with the highest severity. `change` is `:raised` or `:cleared` on the evaluation where the alert starts or stops, and `:none` otherwise, so you can page on edges without keeping your own copy of the state.

`evaluate` answers a `Verdict`, or refuses with `:time_regression` or `:shape`.

### No data

A window reading can be `:no_data`. It also counts as missing when its total is zero, or below the `min_total` you configured.

```ruby
monitor = BM::Monitor.new(BM::Objective.new(999, 1000), :hold, 100)
monitor.add_rule(BM::Rule.new(14_400, 14_400, 2))

decision = monitor.evaluate(t0, [BM::Windows.new(read.(9_800, 10_000), :no_data)]).decisions.first
decision.measured     # => false
decision.state        # => :no_data, it has never been measured
decision.short_milli  # => nil
```

The policy decides what a rule does when it can't be measured:

- `:hold` keeps the state it had. A firing alert keeps firing through a scrape gap.
- `:reset` puts it back to `:no_data`. A firing alert clears, and says so with `:cleared`.
- `:fire` treats silence as an outage.

Whichever you pick, `measured` is `false` on that decision, so a state the policy chose is never mistaken for one the numbers chose.

### Time only moves forward

```ruby
monitor.evaluate(t0 + minute, [windows.(hot, hot)])
monitor.evaluate(t0, [windows.(hot, hot)])   # => :time_regression
monitor.evaluate(t0 + (2 * minute), [])      # => :shape, one Windows per rule
```

A refused evaluation changes no alert state. Evaluating the same instant twice is allowed.

## The Rules of the Tender

- **Time is Integer nanoseconds.** The `now` of `evaluate`, `Decision#since` and `Monitor#last` are instants of a monotonic clock in nanoseconds. A Float or anything else that isn't an Integer raises `TypeError`; negative or past `2**64-1` raises `RangeError`. A ledger period is not time: it is an opaque number you advance.
- **Everything is an Integer.** Counts, periods and instants are in `0..2**64-1`. Objectives and thresholds are in `0..2**32-1`. Severity is `0..255`. Floats, nil and strings raise `TypeError`, and so does an Array where a `Counts`, `Objective`, `Rule`, `Sample::Counts` or `Windows` belongs, or `nil` where a window reading belongs. Out of range, `good > total`, a 100% objective, or a clear threshold above the fire threshold raises `RangeError` when the value is built, so an invalid one never exists.
- **Ruby mirrors Rust.** Every `Data` value has the name and the fields, in order, of the core's struct. An enum's unit variant is a symbol and a variant that carries something is a `Data` named after it: the no-data policy is `:hold`, `:reset` or `:fire`; a rule's state is `:no_data`, `:quiet` or `:firing`; a change is `:none`, `:raised` or `:cleared`; a window reading is `:no_data` or a `Sample::Counts`; a standing is `:no_data`, a `Standing::Within` or a `Standing::Exhausted`.
- **Refusals don't raise.** `:stale_period`, `:overflow`, `:below_observed`, `:full`, `:shape` and `:time_regression` are answers, and the object is exactly as it was before you asked.
- **Every answer, by method.**

  | Method | Honoured | Refused |
  | --- | --- | --- |
  | `Ledger#record(period, counts)` | `:ok` | `:stale_period`, `:overflow` |
  | `Ledger#replace(period, counts)` | `:ok` | `:stale_period` |
  | `Ledger#roll(period)` | the closed `Account` | `:stale_period` |
  | `Ledger#account_against(planned_total)` | an `Account` | `:below_observed` |
  | `Monitor#add_rule(rule)` | the rule's index (Integer) | `:full` |
  | `Monitor#evaluate(now, windows)` | a `Verdict` | `:time_regression`, `:shape` |

  Those six symbols are every variant of the core's `Refusal`, and each is reachable. There is no catch-all symbol.
- **No floats inside.** Thresholds are compared by cross-multiplying in 128 bits. Reported burn rates are rounded down to the thousandth. Decisions never use the rounded number.
- **No data is not zero.** An empty window has a `nil` burn rate. An empty period has no standing.
- **Frozen means frozen.** `freeze` a ledger or a monitor and anything that would change it raises `FrozenError`. Reading still works.

## What You Think You're Doing vs Reality

### You Think: "Zero errors in the window, so the alert can clear."
### Reality: Zero requests in the window, because the load balancer is on fire.

Zero failures out of ten thousand and zero failures out of zero look identical once you've divided. They aren't the same fact. The first is a measurement. The second is the absence of one, and it shows up most reliably during the outages you most wanted to hear about. The monitor keeps them apart and makes you say, once, in the constructor, what silence means for you.

## What's Yours

This is the initial release of a policy core, and the API will change. It does arithmetic and remembers which alerts are firing. It does not collect metrics, run queries, know what time it is, send pages, or decide what a good request is. You bring the counts for each window, exact, from wherever you keep them, and you choose the window lengths. `window_machines` can give you the window boundaries. You decide when a compliance period ends and call `roll`. State lives in your process and is gone when it exits, so persist what you can't afford to recompute. If an empty budget should shed load or open a breaker, that is your coordinator reading an `Account` and telling `shed_machines` or `breaker_machines` what to do. This gem will not do it behind your back.

## License

MIT License. See [LICENSE.txt](LICENSE.txt) for details.

**Remember: the budget is there to be spent. Just not by accident, and not all on a Tuesday.**

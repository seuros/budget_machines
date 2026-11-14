# frozen_string_literal: true
require 'minitest/autorun'
require 'budget_machines'

class MonitorTest < Minitest::Test
  include BudgetMachines

  SLO = Objective.new(999, 1000)
  HOT = Sample::Counts.new(Counts.new(9800, 10_000))
  CALM = Sample::Counts.new(Counts.new(10_000, 10_000))

  def read(good, total) = Sample::Counts.new(Counts.new(good, total))

  def pair(long, short) = Windows.new(long, short)

  def monitor(policy = :hold, min_total = 0)
    Monitor.new(SLO, policy, min_total).tap do |m|
      assert_equal 0, m.add_rule(Rule.new(14_400, 14_400, 2))
      assert_equal 1, m.add_rule(Rule.new(6000, 6000, 1))
    end
  end

  def test_starts_with_no_rules_and_no_evaluation
    m = Monitor.new(SLO, :hold, 0)
    assert_equal [0, true, nil, nil, SLO], [m.size, m.empty?, m.last, m.worst, m.objective]
    assert_nil m.rule(0)
    assert_nil m.state(0)
    m = monitor
    assert_equal [2, false], [m.size, m.empty?]
    assert_equal Rule.new(fire_milli: 6000, clear_milli: 6000, severity: 1), m.rule(1)
    assert_equal [:no_data, :no_data, nil, nil], [m.state(0), m.state(1), m.state(2), m.state(2**64 - 1)]
    assert_nil m.rule(2)
  end

  def test_both_windows_must_burn
    m = monitor
    spike_over = m.evaluate(1, [pair(HOT, CALM), pair(HOT, CALM)])
    assert_instance_of Verdict, spike_over
    assert_equal %i[decisions worst], Verdict.members
    assert_equal %i[quiet quiet], spike_over.decisions.map(&:state)
    assert_nil spike_over.worst
    blip = m.evaluate(2, [pair(CALM, HOT), pair(CALM, HOT)])
    assert_equal %i[none none], blip.decisions.map(&:change)

    verdict = m.evaluate(3, [pair(HOT, HOT), pair(CALM, HOT)])
    page, ticket = verdict.decisions
    assert_equal Decision.new(
      state: :firing, change: :raised, long_milli: 20_000, short_milli: 20_000, since: 3
    ), page
    assert_equal :firing, page.state
    assert_predicate page, :measured?
    assert_predicate verdict, :frozen?
    assert_predicate verdict.decisions, :frozen?
    assert_equal [:quiet, :none, 1], [ticket.state, ticket.change, ticket.since]
    assert_equal [0, 0, 3], [verdict.worst, m.worst, m.last]

    verdict = m.evaluate(4, [pair(HOT, CALM), pair(HOT, HOT)])
    assert_equal %i[cleared raised], verdict.decisions.map(&:change)
    assert_equal [:quiet, :firing, 1, 1], [m.state(0), m.state(1), verdict.worst, m.worst]
  end

  def test_hysteresis
    m = Monitor.new(Objective.new(99, 100), :hold, 0)
    m.add_rule(Rule.new(2000, 1000, 0))
    step = lambda do |now, bad|
      window = read(100 - bad, 100)
      m.evaluate(now, [pair(window, window)]).decisions.first
    end
    assert_equal :quiet, step.(0, 1).state
    assert_equal :raised, step.(1, 2).change
    assert_equal %i[firing none], [step.(2, 1).state, step.(2, 1).change]
    assert_equal :cleared, step.(3, 0).change
    assert_equal :quiet, step.(4, 1).state
  end

  def test_no_data_policies
    hot = pair(read(0, 10), read(0, 10))
    { hold: %i[no_data firing], reset: %i[no_data no_data], fire: %i[firing firing] }
      .each do |policy, (from_start, from_firing)|
      m = Monitor.new(Objective.new(9, 10), policy, 5)
      m.add_rule(Rule.new(1000, 1000, 0))
      first = m.evaluate(0, [pair(:no_data, :no_data)]).decisions.first
      assert_equal [from_start, false, nil, nil], [first.state, first.measured?, first.long_milli, first.short_milli]
      assert_equal :firing, m.evaluate(1, [hot]).decisions.first.state
      thin = m.evaluate(2, [pair(read(0, 10), read(0, 4))]).decisions.first
      assert_equal [from_firing, false, 10_000, nil], [thin.state, thin.measured?, thin.long_milli, thin.short_milli]
      empty = m.evaluate(3, [pair(read(0, 0), read(0, 10))]).decisions.first
      refute_predicate empty, :measured?
    end
  end

  def test_refusals_change_nothing
    m = monitor
    hot = pair(HOT, HOT)
    m.evaluate(10, [hot, hot])
    assert_equal :time_regression, m.evaluate(9, [pair(:no_data, :no_data), pair(:no_data, :no_data)])
    assert_equal :shape, m.evaluate(11, [hot])
    assert_equal :shape, m.evaluate(11, [hot, hot, hot])
    assert_equal :shape, m.evaluate(11, [])
    assert_equal [:firing, :firing, 10, 0], [m.state(0), m.state(1), m.last, m.worst]
    assert_equal %i[none none], m.evaluate(10, [hot, hot]).decisions.map(&:change)
    calm = pair(CALM, CALM)
    assert_equal %i[cleared cleared], m.evaluate(2**64 - 1, [calm, calm]).decisions.map(&:change)
  end

  def test_instants_are_integer_nanoseconds_in_and_out
    m = monitor
    now = Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)
    quiet = [pair(CALM, CALM), pair(:no_data, :no_data)]
    burning = [pair(HOT, HOT), pair(:no_data, :no_data)]
    assert_nil m.last
    assert_equal [now, nil], m.evaluate(now, quiet).decisions.map(&:since)
    assert_equal [now, nil], m.evaluate(now + 1, quiet).decisions.map(&:since)
    page = m.evaluate(now + 60_000_000_000, burning).decisions.first
    assert_equal [:raised, now + 60_000_000_000], [page.change, page.since]
    assert_equal now + 60_000_000_000, m.last
    assert_kind_of Integer, page.since
    assert_equal :time_regression, m.evaluate(now + 59_999_999_999, burning)
    assert_raises(TypeError) { m.evaluate(now.to_f, burning) }
    assert_raises(TypeError) { m.evaluate(now.to_r, burning) }
    assert_raises(RangeError) { m.evaluate(2**64, burning) }
    assert_raises(RangeError) { m.evaluate(-1, burning) }
    assert_equal now + 60_000_000_000, m.last
  end

  def test_capacity
    m = Monitor.new(Objective.new(9, 10), :hold, 0)
    assert_equal 8, m.capacity
    assert_equal Verdict.new(decisions: [], worst: nil), m.evaluate(0, [])
    assert_equal (0..7).to_a, (8.times.map { |i| m.add_rule(Rule.new(1000, 1000, i)) })
    assert_equal :full, m.add_rule(Rule.new(1000, 1000, 0))
    assert_equal 8, m.size
    hot = pair(read(0, 1), read(0, 1))
    verdict = m.evaluate(0, Array.new(8, hot))
    assert_equal [8, 7, 7], [verdict.decisions.size, verdict.worst, m.worst]
  end

  def test_rule_and_windows_are_validated_values
    assert_equal %i[fire_milli clear_milli severity], Rule.members
    assert_equal %i[long short], Windows.members
    assert_raises(RangeError) { Rule.new(0, 0, 0) }
    assert_raises(RangeError) { Rule.new(1000, 1001, 0) }
    assert_raises(RangeError) { Rule.new(1000, 1000, 256) }
    assert_raises(RangeError) { Rule.new(2**32, 1, 0) }
    [1.0, nil, '1'].each do |value|
      assert_raises(TypeError) { Rule.new(value, 1, 0) }
      assert_raises(TypeError) { Rule.new(1, value, 0) }
      assert_raises(TypeError) { Rule.new(1, 1, value) }
    end
    assert_equal %i[counts], Sample::Counts.members
    assert_equal Counts.new(9800, 10_000), HOT.counts
    assert_equal [HOT, :no_data], [Windows.new(HOT, :no_data).long, Windows.new(HOT, :no_data).short]
    assert_raises(TypeError) { Sample::Counts.new([1, 2]) }
    assert_raises(TypeError) { Sample::Counts.new(nil) }
    assert_raises(TypeError) { Windows.new([1, 2], :no_data) }
    assert_raises(TypeError) { Windows.new(HOT, nil) }
    assert_raises(RangeError) { Windows.new(HOT, :quiet) }
    assert_raises(TypeError) { Windows.new(Counts.new(1, 2), HOT) }
    assert_raises(ArgumentError) { Windows.new(HOT) }
  end

  def test_strict_inputs_before_mutation
    m = monitor
    both = [pair(HOT, HOT), pair(HOT, HOT)]
    [1.0, nil, '1', Object.new].each do |value|
      assert_raises(TypeError) { Monitor.new(value, :hold, 0) }
      assert_raises(TypeError) { Monitor.new(SLO, :hold, value) }
      assert_raises(TypeError) { m.add_rule(value) }
      assert_raises(TypeError) { m.evaluate(value, both) }
      assert_raises(TypeError) { m.evaluate(0, value) }
      assert_raises(TypeError) { m.evaluate(0, [pair(HOT, HOT), value]) }
      assert_raises(TypeError) { m.rule(value) }
      assert_raises(TypeError) { m.state(value) }
    end
    assert_raises(TypeError) { m.add_rule([1000, 1000, 0]) }
    assert_raises(TypeError) { m.evaluate(0, [pair(HOT, HOT), [HOT, HOT]]) }
    assert_raises(RangeError) { m.evaluate(-1, both) }
    assert_raises(TypeError) { Monitor.new([999, 1000], :hold, 0) }
    assert_raises(TypeError) { Monitor.new(SLO, 'hold', 0) }
    assert_raises(RangeError) { Monitor.new(SLO, :panic, 0) }
    assert_raises(ArgumentError) { Monitor.new(SLO) }
    assert_raises(ArgumentError) { Monitor.new(SLO, :hold) }
    assert_raises(TypeError) { Monitor.allocate }
    assert_equal [2, :no_data, :no_data, nil], [m.size, m.state(0), m.state(1), m.last]
  end

  def test_freeze_guards
    m = monitor
    m.freeze
    assert_raises(FrozenError) { m.add_rule(Rule.new(1000, 1000, 0)) }
    assert_raises(FrozenError) { m.evaluate(0, [pair(HOT, HOT), pair(HOT, HOT)]) }
    assert_equal %i[no_data no_data], [m.state(0), m.state(1)]
  end

  def test_gc_stress_at_the_integer_limits
    previous = GC.stress
    GC.stress = true
    m = Monitor.new(Objective.new(2**32 - 2, 2**32 - 1), :hold, 2**64 - 1)
    m.add_rule(Rule.new(2**32 - 1, 1, 255))
    all_bad = read(0, 2**64 - 1)
    decision = m.evaluate(2**64 - 1, [pair(all_bad, all_bad)]).decisions.first
    assert_equal [:firing, (2**32 - 1) * 1000, 2**64 - 1], [decision.state, decision.long_milli, decision.since]
    refute m.evaluate(2**64 - 1, [pair(all_bad, read(0, 2**64 - 2))]).decisions.first.measured?
  ensure
    GC.stress = previous
  end
end

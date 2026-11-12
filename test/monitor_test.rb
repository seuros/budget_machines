# frozen_string_literal: true
require 'minitest/autorun'
require 'budget_machines'

class MonitorTest < Minitest::Test
  HOT = [9800, 10_000].freeze
  CALM = [10_000, 10_000].freeze

  def monitor(**options)
    BudgetMachines::Monitor.new(999, 1000, **options).tap do |m|
      assert_equal 0, m.add_rule(14_400, 14_400, 2)
      assert_equal 1, m.add_rule(6000, 6000, 1)
    end
  end

  def test_both_windows_must_burn
    m = monitor
    assert_equal %i[no_data no_data], m.states
    assert_nil m.worst
    assert_nil m.last

    spike_over = m.evaluate(1, [[HOT, CALM], [HOT, CALM]])
    assert_equal %i[quiet quiet], spike_over.map(&:state)
    blip = m.evaluate(2, [[CALM, HOT], [CALM, HOT]])
    assert_equal [nil, nil], blip.map(&:change)

    page, ticket = m.evaluate(3, [[HOT, HOT], [CALM, HOT]])
    assert_equal BudgetMachines::Decision.new(
      state: :firing, change: :raised, measured: true, long_milli: 20_000, short_milli: 20_000, since: 3
    ), page
    assert_predicate page, :firing?
    refute_predicate page, :no_data?
    assert_predicate page, :frozen?
    assert_equal [:quiet, nil, 1], [ticket.state, ticket.change, ticket.since]
    assert_equal 0, m.worst
    assert_equal 3, m.last

    page, ticket = m.evaluate(4, [[HOT, CALM], [HOT, HOT]])
    assert_equal %i[cleared raised], [page.change, ticket.change]
    assert_equal %i[quiet firing], m.states
    assert_equal 1, m.worst
  end

  def test_hysteresis
    m = BudgetMachines::Monitor.new(99, 100)
    m.add_rule(2000, 1000, 0)
    step = ->(now, bad) { m.evaluate(now, [[[100 - bad, 100], [100 - bad, 100]]]).first }
    assert_equal :quiet, step.(0, 1).state
    assert_equal :raised, step.(1, 2).change
    assert_equal [:firing, nil], [step.(2, 1).state, step.(2, 1).change]
    assert_equal :cleared, step.(3, 0).change
    assert_equal :quiet, step.(4, 1).state
  end

  def test_no_data_policies
    hot = [[0, 10], [0, 10]]
    { hold: %i[no_data firing], reset: %i[no_data no_data], fire: %i[firing firing] }
      .each do |policy, (from_start, from_firing)|
      m = BudgetMachines::Monitor.new(9, 10, on_no_data: policy, min_total: 5)
      m.add_rule(1000, 1000, 0)
      first = m.evaluate(0, [[nil, nil]]).first
      assert_equal [from_start, false, nil, nil], [first.state, first.measured, first.long_milli, first.short_milli]
      assert_predicate first, :no_data?
      assert_predicate m.evaluate(1, [hot]).first, :firing?
      thin = m.evaluate(2, [[[0, 10], [0, 4]]]).first
      assert_equal [from_firing, false, 10_000, nil], [thin.state, thin.measured, thin.long_milli, thin.short_milli]
      empty = m.evaluate(3, [[[0, 0], [0, 10]]]).first
      refute empty.measured
    end
  end

  def test_refusals_change_nothing
    m = monitor
    hot = [HOT, HOT]
    m.evaluate(10, [hot, hot])
    assert_equal :time_regression, m.evaluate(9, [[nil, nil], [nil, nil]])
    assert_equal :shape, m.evaluate(11, [hot])
    assert_equal :shape, m.evaluate(11, [hot, hot, hot])
    assert_equal :shape, m.evaluate(11, [])
    assert_equal [%i[firing firing], 10, 0], [m.states, m.last, m.worst]
    assert_equal [nil, nil], m.evaluate(10, [hot, hot]).map(&:change)
    assert_equal %i[cleared cleared], m.evaluate(2**64 - 1, [[CALM, CALM], [CALM, CALM]]).map(&:change)
  end

  def test_capacity
    m = BudgetMachines::Monitor.new(9, 10)
    assert_equal 8, m.capacity
    assert_equal 0, m.size
    assert_equal [], m.evaluate(0, [])
    assert_equal (0..7).to_a, (8.times.map { |i| m.add_rule(1000, 1000, i) })
    assert_equal :full, m.add_rule(1000, 1000, 0)
    assert_equal 8, m.size
    hot = [[0, 1], [0, 1]]
    assert_equal 8, m.evaluate(0, Array.new(8, hot)).size
    assert_equal 7, m.worst
  end

  def test_strict_inputs_before_mutation
    m = monitor
    [1.0, nil, '1', Object.new].each do |value|
      assert_raises(TypeError) { BudgetMachines::Monitor.new(value, 10) }
      assert_raises(TypeError) { BudgetMachines::Monitor.new(9, 10, min_total: value) } unless value.nil?
      assert_raises(TypeError) { m.add_rule(value, 1, 0) }
      assert_raises(TypeError) { m.add_rule(1, value, 0) }
      assert_raises(TypeError) { m.add_rule(1, 1, value) }
      assert_raises(TypeError) { m.evaluate(value, [[HOT, HOT], [HOT, HOT]]) }
      assert_raises(TypeError) { m.evaluate(0, value) }
      assert_raises(TypeError) { m.evaluate(0, [[HOT, HOT], value]) }
      assert_raises(TypeError) { m.evaluate(0, [[HOT, HOT], [HOT, [value, 1]]]) } unless value.nil?
    end
    assert_raises(TypeError) { m.evaluate(0, [[HOT, HOT], [HOT, [1]]]) }
    assert_raises(TypeError) { m.evaluate(0, [[HOT, HOT], [HOT]]) }
    assert_raises(TypeError) { m.evaluate(0, [[HOT, HOT], [HOT, [nil, 1]]]) }
    assert_raises(RangeError) { m.evaluate(0, [[HOT, HOT], [HOT, [2, 1]]]) }
    assert_raises(RangeError) { m.evaluate(-1, [[HOT, HOT], [HOT, HOT]]) }
    assert_raises(RangeError) { m.add_rule(0, 0, 0) }
    assert_raises(RangeError) { m.add_rule(1000, 1001, 0) }
    assert_raises(RangeError) { m.add_rule(1000, 1000, 256) }
    assert_raises(RangeError) { m.add_rule(2**32, 1, 0) }
    assert_raises(RangeError) { BudgetMachines::Monitor.new(10, 10) }
    assert_raises(TypeError) { BudgetMachines::Monitor.new(9, 10, on_no_data: 'hold') }
    assert_raises(ArgumentError) { BudgetMachines::Monitor.new(9, 10, on_no_data: :panic) }
    assert_raises(ArgumentError) { BudgetMachines::Monitor.new(9, 10, nope: 1) }
    assert_raises(TypeError) { BudgetMachines::Monitor.allocate }
    assert_equal [2, %i[no_data no_data], nil], [m.size, m.states, m.last]
  end

  def test_freeze_guards
    m = monitor
    m.freeze
    assert_raises(FrozenError) { m.add_rule(1000, 1000, 0) }
    assert_raises(FrozenError) { m.evaluate(0, [[HOT, HOT], [HOT, HOT]]) }
    assert_equal %i[no_data no_data], m.states
  end

  def test_gc_stress_at_the_integer_limits
    previous = GC.stress
    GC.stress = true
    m = BudgetMachines::Monitor.new(2**32 - 2, 2**32 - 1, min_total: 2**64 - 1)
    m.add_rule(2**32 - 1, 1, 255)
    all_bad = [0, 2**64 - 1]
    decision = m.evaluate(2**64 - 1, [[all_bad, all_bad]]).first
    assert_equal [:firing, (2**32 - 1) * 1000, 2**64 - 1], [decision.state, decision.long_milli, decision.since]
    refute m.evaluate(2**64 - 1, [[all_bad, [0, 2**64 - 2]]]).first.measured
  ensure
    GC.stress = previous
  end
end

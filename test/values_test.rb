# frozen_string_literal: true
require 'minitest/autorun'
require 'budget_machines'

# Every Data value validates through the core, whether the gem or a caller
# built it, and the binding raises nothing after the core has changed.
class ValuesTest < Minitest::Test
  include BudgetMachines

  SLO = Objective.new(99, 100)
  HOT = Sample::Counts.new(Counts.new(9800, 10_000))

  def rebuilt(value) = value.class.new(*value.deconstruct)

  def test_returned_values_are_accepted_back
    ledger = Ledger.new(SLO, 1)
    [Counts.new(0, 0), Counts.new(995, 1000), Counts.new(900, 1000)].each do |reading|
      ledger.replace(1, reading)
      [ledger.account, ledger.account_against(5000)].each do |account|
        assert_equal account, rebuilt(account)
        assert_equal account.standing, rebuilt(account.standing) unless account.standing == :no_data
      end
    end
    monitor = Monitor.new(Objective.new(999, 1000), :fire, 0)
    monitor.add_rule(Rule.new(14_400, 14_400, 1))
    [Windows.new(HOT, HOT), Windows.new(:no_data, HOT)].each_with_index do |windows, now|
      verdict = monitor.evaluate(now, [windows])
      assert_equal verdict, rebuilt(verdict)
      verdict.decisions.each { |decision| assert_equal decision, rebuilt(decision) }
      assert_predicate verdict.decisions, :frozen?
    end
  end

  def test_accounts_no_ledger_produces_raise_range_error
    five = Counts.new(4, 5)
    within = Standing::Within.new(0)
    assert_equal within, Account.new(1, five, 5, 1, within, 1000).standing
    [
      [1, five, 4, 1, within, 1000], # base below the observed total
      [1, five, 5, 6, Standing::Within.new(5), 166], # allowance above the base
      [1, five, 5, 2, within, 500], # standing disagrees with the counts
      [1, five, 5, 1, :no_data, 1000],
      [1, five, 5, 1, within, nil], # consumption without a base, or beyond the allowance
      [1, five, 5, 1, within, 1001],
      [1, five, 5, 1, :exhausted, 1000]
    ].each { |fields| assert_raises(RangeError) { Account.new(*fields) } }
    assert_raises(TypeError) { Account.new(1, [4, 5], 5, 1, within, 1000) }
    assert_raises(TypeError) { Account.new(1, five, 5, 1, 'within', 1000) }
    assert_raises(TypeError) { Account.new(1, five, 5.0, 1, within, 1000) }
    assert_raises(RangeError) { Standing::Within.new(-1) }
    assert_raises(RangeError) { Standing::Exhausted.new(2**64) }
    assert_raises(TypeError) { Standing::Exhausted.new(1.0) }
  end

  def test_decisions_and_verdicts_no_evaluation_produces_raise_range_error
    quiet = Decision.new(:quiet, :none, 1, 2, nil)
    firing = Decision.new(:firing, :raised, nil, nil, 7)
    assert_equal [true, false], [quiet.measured?, firing.measured?]
    [
      [:quiet, :raised, 1, 2, 7], # an edge into the wrong state
      [:firing, :raised, 1, 2, nil], # an edge without its instant
      [:no_data, :none, 1, 2, nil], # both rates measured, yet no data
      [:asleep, :none, nil, nil, nil],
      [:quiet, :maybe, nil, nil, nil],
      [:quiet, :none, nil, nil, -1]
    ].each { |fields| assert_raises(RangeError) { Decision.new(*fields) } }
    assert_raises(TypeError) { Decision.new('quiet', :none, nil, nil, nil) }
    assert_raises(TypeError) { Decision.new(:quiet, :none, 1.0, nil, nil) }
    assert_equal 1, Verdict.new([quiet, firing], 1).worst
    assert_raises(RangeError) { Verdict.new([quiet, firing], nil) }
    assert_raises(RangeError) { Verdict.new([quiet, firing], 0) }
    assert_raises(RangeError) { Verdict.new([quiet], 3) }
    assert_raises(RangeError) { Verdict.new([quiet] * (CAPACITY + 1), nil) }
    assert_raises(TypeError) { Verdict.new([quiet, :firing], 1) }
    assert_raises(TypeError) { Verdict.new(quiet, nil) }
  end

  VALUES = [
    Counts, Objective, Standing::Within, Standing::Exhausted, Account, Sample::Counts, Windows, Rule,
    Decision, Verdict
  ].freeze

  def test_value_classes_are_sealed
    VALUES.each do |value|
      assert_raises(NoMethodError) { value.allocate }
      assert_raises(TypeError) { Class.new(value) }
    end
    assert_equal Counts.new(1, 2), Counts.new(1, 1).with(total: 2)
    assert_raises(RangeError) { Counts.new(1, 1).with(good: 2) }
  end

  # Sealing stops accidents, not reflection: a named subclass raises, yet
  # Ruby has already bound its constant, and `Data#initialize` can be bound
  # to skip the validating one. The binding refuses such a value by its
  # exact class before it reaches the core.
  def test_a_leaked_subclass_never_reaches_the_core
    assert_raises(TypeError) { eval('class ForgedCounts < BudgetMachines::Counts; end', binding, __FILE__, __LINE__) }
    assert_equal 'constant', defined?(ForgedCounts) # the constant leaks
    ForgedCounts.define_method(:initialize) { |**kw| Data.instance_method(:initialize).bind_call(self, **kw) }
    forged = ForgedCounts.new(good: 5, total: 1)
    ledger = Ledger.new(SLO, 1)
    assert_raises(TypeError) { ledger.record(1, forged) }
    assert_raises(TypeError) { Counts.new(1, 1).checked_add(forged) }
    assert_raises(TypeError) { SLO.burn_milli(forged) }
    assert_equal Counts.new(0, 0), ledger.counts
  ensure
    self.class.send(:remove_const, :ForgedCounts) if self.class.const_defined?(:ForgedCounts, false)
  end

  # `Marshal.load` builds an instance of the real class without `initialize`;
  # the binding rebuilds every value it reads through the core.
  # A singleton class opened on a value does not change what it is.
  def test_an_opened_singleton_class_is_still_the_gems_value
    reading = Counts.new(9, 10).tap(&:singleton_class)
    assert_equal :ok, Ledger.new(SLO, 1).record(1, reading)
  end

  def test_a_marshal_forged_value_never_reaches_the_core
    ledger = Ledger.new(SLO, 1)
    assert_raises(RangeError) { ledger.record(1, forged(Counts, 5, 1)) }
    assert_raises(RangeError) { Ledger.new(forged(Objective, 100, 100), 1) }
    monitor = Monitor.new(SLO, :hold, 0)
    assert_raises(RangeError) { monitor.add_rule(forged(Rule, 1, 2, 0)) }
    hot = forged(Sample::Counts, forged(Counts, 3, 2))
    monitor.add_rule(Rule.new(1000, 1000, 0))
    assert_raises(RangeError) { monitor.evaluate(1, [forged(Windows, hot, hot)]) }
    assert_raises(RangeError) { Verdict.new([forged(Decision, :quiet, :raised, 1, 2, 7)], nil) }
    assert_equal [Counts.new(0, 0), 1, nil], [ledger.counts, monitor.size, monitor.last]
  end

  def test_roll_applies_nothing_when_building_its_answer_raises
    ledger = recorded_ledger
    replacing(Account, ->(*) { raise IOError }) { assert_raises(IOError) { ledger.roll(2) } }
    assert_equal [1, Counts.new(9, 10)], [ledger.period, ledger.counts]
  end

  def test_roll_applies_nothing_when_building_its_answer_re_enters
    ledger = recorded_ledger
    replacing(Account, ->(*) { ledger.record(1, Counts.new(1, 1)) }) do
      assert_raises(RuntimeError) { ledger.roll(2) }
    end
    assert_equal [1, Counts.new(10, 11)], [ledger.period, ledger.counts]
  end

  def test_roll_applies_nothing_when_building_its_answer_freezes_the_ledger
    ledger = recorded_ledger
    replacing(Account, ->(*) { ledger.freeze }) { assert_raises(FrozenError) { ledger.roll(2) } }
    assert_equal [1, Counts.new(9, 10)], [ledger.period, ledger.counts]
  end

  def test_evaluate_applies_nothing_when_building_its_answer_raises_re_enters_or_freezes
    [
      [IOError, ->(_) { raise IOError }],
      [RuntimeError, ->(monitor) { monitor.evaluate(9, [Windows.new(HOT, HOT)]) }],
      [FrozenError, lambda(&:freeze)]
    ].each do |error, hook|
      monitor = Monitor.new(SLO, :hold, 0)
      monitor.add_rule(Rule.new(1000, 1000, 0))
      replacing(Verdict, ->(*) { hook.call(monitor) }) do
        assert_raises(error) { monitor.evaluate(5, [Windows.new(HOT, HOT)]) }
      end
      expected = error == RuntimeError ? [9, :firing] : [nil, :no_data]
      assert_equal expected, [monitor.last, monitor.state(0)]
    end
  end

  # No borrow is held while a reader builds its answer, so a re-entrant call
  # from `new` works instead of aborting the process.
  def test_readers_hold_no_borrow_while_building
    ledger = recorded_ledger
    replacing(Objective, ->(*) { ledger.record(1, Counts.new(1, 1)) }) { ledger.objective }
    replacing(Counts, ->(*) { ledger.replace(1, Counts.new(0, 0)) }) { ledger.counts }
    assert_equal Counts.new(0, 0), ledger.counts
  end

  # The classes are resolved once and pinned: after a compaction that moves
  # everything movable, every path that checks or builds a value still finds
  # them (unpinned, the exact-class check would refuse the gem's own values).
  def test_classes_survive_gc_compaction
    if GC.respond_to?(:verify_compaction_references)
      GC.verify_compaction_references(expand_heap: true, toward: :empty)
    elsif GC.respond_to?(:compact)
      GC.compact
    end
    ledger = recorded_ledger
    assert_equal [SLO, Counts.new(9, 10)], [ledger.objective, ledger.counts]
    assert_equal Counts.new(2, 3), Counts.new(1, 1).checked_add(Counts.new(1, 2))
    assert_kind_of Account, ledger.account
    assert_kind_of Account, ledger.account_against(100)
    exhausted = Ledger.new(SLO, 1).tap { |l| l.record(1, Counts.new(0, 10)) }.account.standing
    assert_instance_of Standing::Exhausted, exhausted
    assert_instance_of Account, ledger.roll(2)
    monitor = Monitor.new(SLO, :hold, 0)
    monitor.add_rule(Rule.new(1000, 1000, 0))
    assert_equal Rule.new(1000, 1000, 0), monitor.rule(0)
    assert_instance_of Verdict, monitor.evaluate(1, [Windows.new(HOT, :no_data)])
    assert_instance_of Decision, monitor.evaluate(2, [Windows.new(HOT, HOT)]).decisions.first
  end

  private

  def recorded_ledger
    Ledger.new(SLO, 1).tap { |ledger| ledger.record(1, Counts.new(9, 10)) }
  end

  # Runs the block with `klass.new` calling `hook` first, once; the original
  # `new` is restored afterwards.
  def replacing(klass, hook)
    original = klass.method(:new)
    armed = true
    klass.define_singleton_method(:new) do |*args, **kw|
      if armed
        armed = false
        hook.call(*args)
      end
      original.call(*args, **kw)
    end
    yield
  ensure
    klass.singleton_class.send(:remove_method, :new)
    klass.define_singleton_method(:new, original)
  end

  # An instance of `klass` holding `fields`, loaded by Marshal from a look-alike
  # so its `initialize` never ran.
  def forged(klass, *fields)
    stand_in = Data.define(*klass.members)
    Object.const_set(:MarshalStandIn, stand_in)
    bytes = Marshal.dump(stand_in.new(*fields))
    name = klass.name
    Marshal.load(bytes.sub(":#{(5 + 14).chr}MarshalStandIn".b, ":#{(5 + name.size).chr}#{name}".b))
  ensure
    Object.send(:remove_const, :MarshalStandIn)
  end
end

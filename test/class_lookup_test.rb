# frozen_string_literal: true

require 'minitest/autorun'
require 'open3'
require 'rbconfig'

# The classes of the values the extension wraps (Ledger, Monitor) are fixed
# when it loads. Nothing a caller does later, redefining a constant or
# `Object.const_get`, reaches a value being built. Each case runs in a fresh
# process: the class is fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'budget_machines'
    B = BudgetMachines
    SLO = B::Objective.new(999, 1000)
    RULE = B::Rule.new(14_400, 14_400, 2)
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a Ledger or Monitor constant redefined before the first `new` made
  # the value an instance of the new class, without the gem's methods.
  def test_a_redefined_constant_does_not_change_the_value_class
    assert_equal 'true 60 true 0', isolated(<<~RUBY)
      ledger_class = B::Ledger
      monitor_class = B::Monitor
      B.send(:remove_const, :Ledger)
      B.const_set(:Ledger, Class.new)
      B.send(:remove_const, :Monitor)
      B.const_set(:Monitor, Class.new)
      ledger = ledger_class.new(SLO, 60)
      monitor = monitor_class.new(SLO, :hold, 0)
      puts [ledger.instance_of?(ledger_class), ledger.period,
            monitor.instance_of?(monitor_class), monitor.add_rule(RULE)].join(' ')
    RUBY
  end

  # Bug: building the first value ran `Object.const_get`, Ruby code in the
  # middle of a native constructor.
  def test_building_a_value_runs_no_const_get
    assert_equal '0 Ledger Monitor', isolated(<<~RUBY)
      hits = 0
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        super(*args)
      end
      built = [B::Ledger.new(SLO, 60), B::Monitor.new(SLO, :hold, 0)]
      puts [hits, *built.map { |value| value.class.name.split('::').last }].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal '60 0', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran in a constructor' }
      ledger = B::Ledger.new(SLO, 60)
      monitor = B::Monitor.new(SLO, :hold, 0)
      puts [ledger.period, monitor.add_rule(RULE)].join(' ')
    RUBY
  end
end

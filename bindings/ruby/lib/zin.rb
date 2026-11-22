# frozen_string_literal: true
require "json"
require "open3"

module Zin
  class Divergence < StandardError; end

  # Fresh adapters implement call(action, named_arguments) returning true or false.
  # observe and close are optional. Exceptions propagate as test errors.
  class Plan
    attr_reader :tests, :system

    def self.load(path, **options)
      if File.extname(path) == ".json"
        raise ArgumentError, "plan options require a .zin source" unless options.empty?
        return new(JSON.parse(File.read(path)))
      end
      flags = options.flat_map do |key, value|
        raise ArgumentError, "unknown plan option #{key}" unless %i[cover max_steps step_budget pair_limit component].include?(key)
        ["--#{key.to_s.tr('_', '-')}", value.to_s]
      end
      out, err, status = Open3.capture3(ENV.fetch("ZIN_BIN", "zin"), "plan", path.to_s, *flags)
      raise ArgumentError, err unless status.success?
      new(JSON.parse(out))
    end

    def initialize(data)
      raise ArgumentError, "expected plan/1" unless data.fetch("zin") == "plan/1"
      @system = data.fetch("system")
      @fingerprint = data.fetch("fingerprint")
      @parents = data.fetch("parents")
      @tests = data.fetch("tests")
      @states = data.fetch("states")
      params = data.fetch("actions").to_h { |a| [a.fetch("name"), a.fetch("params").map { |p| p.fetch("name") }] }
      @calls = data.fetch("calls").map do |c|
        [c.fetch("action"), params.fetch(c.fetch("action")).zip(c.fetch("args")).to_h, c.fetch("text")]
      end
    end

    def run(test, &factory)
      steps = test.fetch("steps")
      failure = replay(steps, factory)
      return unless failure

      index, before, detail = failure
      prefix = steps.first(index)
      report = "#{system}, #{test.fetch('name')}, step #{index}: #{detail}\nfingerprint: #{@fingerprint}\n#{scenario(prefix)}"
      if index.positive?
        candidate = shortest(before) + [prefix.last]
        if candidate.length < index
          begin
            repeated = replay(candidate, factory)
            if repeated == [candidate.length, before, detail]
              report += "\nshortest reproduction:\n#{scenario(candidate)}"
            end
          rescue StandardError => error
            report += "\nshorter replay raised #{error.class}: #{error.message}"
          end
        end
      end
      raise Divergence, report
    end

    private

    def replay(steps, factory)
      adapter = factory.call
      state = 0
      begin
        initial = observation(adapter, state)
        return [0, state, initial] if initial
        steps.each_with_index do |(call_id, to), i|
          name, args, text = @calls.fetch(call_id)
          before = state
          actual = adapter.call(name, args.dup)
          raise TypeError, "#{name} must return true or false" unless [true, false].include?(actual)
          if actual != (to >= 0)
            return [i + 1, before, "#{text} was #{actual ? 'accepted' : 'rejected'}, expected #{to >= 0 ? 'accepted' : 'rejected'}"]
          end
          state = to if to >= 0
          detail = observation(adapter, state)
          return [i + 1, before, detail] if detail
        end
        nil
      ensure
        adapter.close if adapter.respond_to?(:close)
      end
    end

    def shortest(state)
      path = []
      until state.zero?
        parent, call_id = @parents.fetch(state)
        raise ArgumentError, "invalid shortest-path parent" unless parent.is_a?(Integer) && parent.between?(0, state - 1)
        path << [call_id, state]
        state = parent
      end
      path.reverse
    end

    def scenario(steps)
      (['scenario "Reproduction"'] + steps.map do |call_id, to|
        "  #{to == -1 ? 'reject ' : ''}#{@calls.fetch(call_id)[2]}"
      end).join("\n")
    end

    def matches?(expected, actual)
      if expected.is_a?(Hash)
        actual.is_a?(Hash) && actual.all? { |k, v| expected.key?(k) && matches?(expected[k], v) }
      else
        expected.eql?(actual)
      end
    end

    def observation(adapter, state)
      return unless adapter.respond_to?(:observe)
      actual = adapter.observe
      return if actual.nil?
      expected = @states.fetch(state)
      "observed #{actual.inspect}, expected a subset of #{expected.inspect}" unless matches?(expected, actual)
    end
  end

  def self.verify(path, **options, &factory)
    plan = Plan.load(path, **options)
    RSpec.describe(plan.system) do
      plan.tests.each_with_index do |test, i|
        it("#{i + 1}. #{test.fetch('name')}") { plan.run(test, &factory) }
      end
    end
  end
end

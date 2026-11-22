require_relative "../lib/zin"

class Latch
  def initialize
    @locked = false
  end

  def call(action, _args)
    target = action == "lock"
    return false if @locked == target
    @locked = target
    true
  end

  def observe
    { "locked" => @locked }
  end
end

source = File.expand_path("../../fixtures/latch.zin", __dir__)
Zin.verify(source) { Latch.new }

RSpec.describe Zin::Plan do
  it "reports a broken refusal with the exact prefix" do
    plan = Zin::Plan.load(source)
    adapter = Latch.new
    def adapter.call(action, args)
      super
      true
    end
    expect { plan.run(plan.tests.first) { adapter } }.to raise_error(Zin::Divergence, /reject lock/)
  end
end

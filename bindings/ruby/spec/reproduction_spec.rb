require_relative "../lib/zin"

RSpec.describe "verified reproductions" do
  [false, true].each do |different|
    it "retains the original and checks the shorter failure (different=#{different})" do
      plan = Zin::Plan.load(File.expand_path("../../fixtures/latch.zin", __dir__))
      factories = 0
      adapter = Class.new do
        def initialize(different)
          @locked = false
          @different = different
        end

        def call(name, _args)
          return @different if name == "unlock"
          return false if @locked
          @locked = true
        end

        def observe
          { "locked" => @locked }
        end
      end
      expect {
        plan.run(plan.tests.first) { factories += 1; adapter.new(different && factories == 2) }
      }.to raise_error(Zin::Divergence) { |error|
        expect(error.message).to match(/fingerprint: [0-9a-f]{16}/)
        expect(error.message).to include("unlock was rejected, expected accepted")
        expect(error.message).to include("scenario \"Reproduction\"\n  lock\n  reject lock\n  unlock")
        if different
          expect(error.message).not_to include("shortest reproduction:")
        else
          expect(error.message).to end_with("shortest reproduction:\nscenario \"Reproduction\"\n  lock\n  unlock")
        end
      }
      expect(factories).to eq(2)
    end
  end
end

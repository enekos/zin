require_relative "../lib/zin"

RSpec.describe "shared observation contract" do
  fixtures = JSON.parse(File.read(File.expand_path("../../../tests/fixtures/observations.json", __dir__)))
  fixtures.each do |row|
    it row.fetch("name") do
      test = { "name" => "initial observation", "steps" => [] }
      plan = Zin::Plan.new({
        "zin" => "plan/1", "system" => "Observations", "states" => [row.fetch("expected")],
        "tests" => [test], "calls" => [], "actions" => []
      })
      check = proc { plan.run(test) { Struct.new(:observe).new(row.fetch("observed")) } }
      if row.fetch("matches")
        expect(&check).not_to raise_error
      else
        expect(&check).to raise_error(Zin::Divergence)
      end
    end
  end
end

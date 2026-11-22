require "zin"

class Zin$MODELHarness
  def call(action, args)
    # Return true for acceptance and false for a domain refusal.
$ACTIONS    raise ArgumentError, "unknown action #{action}"
  end

  # Return any observable subset of $FIELDS.
  def observe
    {}
  end
end

Zin.verify(File.join(__dir__, $SPEC)) { Zin$MODELHarness.new }

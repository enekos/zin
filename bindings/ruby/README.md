# Zin for RSpec

From this directory, build the gem with `gem build zin-verify.gemspec` and install the local gem. In an RSpec file, require `zin` and register a fresh adapter factory.

```ruby
Zin.verify(File.join(__dir__, "latch.zin"), cover: "pairs") { LatchAdapter.new }
```

Each generated test is a native RSpec example. The adapter's `call(name, args)` must return a boolean. Named argument keys are strings. Optionally implement `observe` to return a partial state with string keys and `close` for cleanup. Unexpected exceptions fail the example.

A source path invokes `ZIN_BIN` or `zin` on PATH. Supplying a generated JSON plan instead replays those exact bytes offline. Regenerate the plan in CI when using this form. The included example and broken-refusal check run with `rspec spec`.

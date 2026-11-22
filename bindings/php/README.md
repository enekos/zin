# Zin for PHPUnit

Install this directory as a Composer path repository or load `src/Plan.php` directly. `Plan::load($spec)` derives a plan through `ZIN_BIN` or `zin` on PATH. A `.json` path explicitly selects an offline plan. Regenerate committed plans in CI when sources change.

A static PHPUnit data provider delegates to `$plan->cases()`. In the test call `$plan->run($index, fn () => new MyAdapter())` and count the passing conformance assertion. Each named data set can be filtered in PHPUnit. The factory runs once per case. The adapter's `call(string $name, array $args)` returns a boolean. Optional `observe()` returns a partial associative array and `close()` releases the implementation. Unexpected exceptions propagate. The [complete consumer](tests/LatchTest.php) includes a failing-refusal regression check.

<?php declare(strict_types=1);

namespace Zin;

use InvalidArgumentException;
use RuntimeException;
use stdClass;

final class Plan
{
    private array $calls = [];

    private function __construct(private readonly stdClass $data)
    {
        if (($data->zin ?? null) !== 'plan/1' || !is_array($data->tests ?? null)) {
            throw new InvalidArgumentException('Expected a Zin plan/1 document');
        }
        $parameters = [];
        foreach ($data->actions as $action) {
            $parameters[$action->name] = array_column($action->params, 'name');
        }
        foreach ($data->calls as $call) {
            $this->calls[] = [$call->action, array_combine($parameters[$call->action], $call->args), $call->text];
        }
    }

    public static function load(string $path, array $options = []): self
    {
        if (str_ends_with($path, '.json')) {
            if ($options !== []) {
                throw new InvalidArgumentException('Plan options require a .zin source');
            }
            $text = file_get_contents($path);
            if ($text === false) {
                throw new RuntimeException("Cannot read $path");
            }
        } else {
            $args = [getenv('ZIN_BIN') ?: 'zin', 'plan', $path];
            foreach ($options as $key => $value) {
                if (!in_array($key, ['cover', 'max-steps', 'step-budget', 'pair-limit', 'component'], true)) {
                    throw new InvalidArgumentException("Unknown plan option $key");
                }
                array_push($args, "--$key", (string)$value);
            }
            $errors = tmpfile();
            try {
                $process = proc_open($args, [1 => ['pipe', 'w'], 2 => $errors], $pipes);
                if (!is_resource($process)) {
                    throw new RuntimeException('Cannot start zin');
                }
                $text = stream_get_contents($pipes[1]);
                fclose($pipes[1]);
                if (proc_close($process) !== 0) {
                    rewind($errors);
                    throw new RuntimeException(stream_get_contents($errors));
                }
            } finally {
                fclose($errors);
            }
        }
        return new self(json_decode($text, false, 512, JSON_THROW_ON_ERROR));
    }

    /** Pass these indexed, named data sets to a PHPUnit data provider. */
    public function cases(): iterable
    {
        foreach ($this->data->tests as $index => $test) {
            yield ($index + 1) . '. ' . $test->name => [$index];
        }
    }

    /** The factory creates an adapter with call(), and optional observe() and close(). */
    public function run(int $index, callable $factory): int
    {
        $test = $this->data->tests[$index] ?? throw new InvalidArgumentException("Unknown case $index");
        $failure = $this->replay($test->steps, $factory);
        if ($failure === null) {
            return count($test->steps);
        }
        [$failed, $before, $detail] = $failure;
        $prefix = array_slice($test->steps, 0, $failed);
        $message = "$test->name, step $failed: $detail\nfingerprint: {$this->data->fingerprint}\n" . $this->scenario($prefix);
        if ($failed > 0) {
            $candidate = $this->shortest($before);
            $candidate[] = $prefix[$failed - 1];
            if (count($candidate) < $failed) {
                try {
                    $again = $this->replay($candidate, $factory);
                    if ($again === [count($candidate), $before, $detail]) {
                        $message .= "\nshortest reproduction:\n" . $this->scenario($candidate);
                    }
                } catch (\Throwable $e) {
                    $message .= "\nshorter replay raised " . get_class($e) . ': ' . $e->getMessage();
                }
            }
        }
        throw new RuntimeException($message);
    }

    private function replay(array $steps, callable $factory): ?array
    {
        $adapter = $factory();
        $state = 0;
        try {
            $detail = $this->observation($adapter, $state);
            if ($detail !== null) {
                return [0, $state, $detail];
            }
            foreach ($steps as $index => [$callId, $to]) {
                [$name, $args, $text] = $this->calls[$callId];
                $before = $state;
                $accepted = $adapter->call($name, $args);
                if (!is_bool($accepted)) {
                    throw new InvalidArgumentException("$name must return a boolean");
                }
                if ($accepted !== ($to >= 0)) {
                    $actual = $accepted ? 'accepted' : 'rejected';
                    $expected = $to >= 0 ? 'accepted' : 'rejected';
                    return [$index + 1, $before, "$text was $actual, expected $expected"];
                }
                if ($to >= 0) $state = $to;
                $detail = $this->observation($adapter, $state);
                if ($detail !== null) {
                    return [$index + 1, $before, $detail];
                }
            }
            return null;
        } finally {
            if (method_exists($adapter, 'close')) $adapter->close();
        }
    }

    private function observation(object $adapter, int $state): ?string
    {
        if (!method_exists($adapter, 'observe') || ($actual = $adapter->observe()) === null) {
            return null;
        }
        $expected = $this->data->states[$state];
        return self::matches($expected, $actual) ? null
            : 'observed ' . json_encode($actual, JSON_THROW_ON_ERROR) . ', expected a subset of ' . json_encode($expected, JSON_THROW_ON_ERROR);
    }

    private function shortest(int $state): array
    {
        $path = [];
        while ($state !== 0) {
            [$parent, $call] = $this->data->parents[$state];
            if (!is_int($parent) || $parent < 0 || $parent >= $state) {
                throw new InvalidArgumentException('invalid shortest-path parent');
            }
            $path[] = [$call, $state];
            $state = $parent;
        }
        return array_reverse($path);
    }

    private function scenario(array $steps): string
    {
        $lines = ['scenario "Reproduction"'];
        foreach ($steps as [$id, $to]) {
            $lines[] = '  ' . ($to === -1 ? 'reject ' : '') . $this->calls[$id][2];
        }
        return implode("\n", $lines);
    }

    private static function matches(mixed $expected, mixed $actual): bool
    {
        if (!$expected instanceof stdClass) {
            return $expected === $actual;
        }
        if (!is_array($actual) && !$actual instanceof stdClass) {
            return false;
        }
        foreach ($actual as $key => $value) {
            $field = (string)$key;
            if (!property_exists($expected, $field) || !self::matches($expected->$field, $value)) {
                return false;
            }
        }
        return true;
    }

}

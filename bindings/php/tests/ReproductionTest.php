<?php declare(strict_types=1);

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Zin\Plan;

require_once __DIR__ . '/../src/Plan.php';

final class ReproductionTest extends TestCase
{
    public static function outcomes(): iterable
    {
        yield [false];
        yield [true];
    }

    #[DataProvider('outcomes')]
    public function testVerifiedShorterScenario(bool $different): void
    {
        $plan = Plan::load(__DIR__ . '/../../fixtures/latch.zin');
        $factories = 0;
        try {
            $plan->run(0, static function () use (&$factories, $different) {
                ++$factories;
                return new class($different && $factories === 2) {
                    private bool $locked = false;
                    public function __construct(private readonly bool $different) {}
                    public function call(string $name, array $args): bool
                    {
                        if ($name === 'unlock') return $this->different;
                        if ($this->locked) return false;
                        $this->locked = true;
                        return true;
                    }
                    public function observe(): array { return ['locked' => $this->locked]; }
                };
            });
            self::fail('Expected a divergence');
        } catch (RuntimeException $e) {
            $report = $e->getMessage();
            self::assertMatchesRegularExpression('/fingerprint: [0-9a-f]{16}/', $report);
            self::assertStringContainsString('unlock was rejected, expected accepted', $report);
            self::assertStringContainsString("scenario \"Reproduction\"\n  lock\n  reject lock\n  unlock", $report);
            if ($different) {
                self::assertStringNotContainsString('shortest reproduction:', $report);
            } else {
                self::assertStringEndsWith("shortest reproduction:\nscenario \"Reproduction\"\n  lock\n  unlock", $report);
            }
        }
        self::assertSame(2, $factories);
    }
}

<?php declare(strict_types=1);

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Zin\Plan;

final class Zin$MODELHarness
{
    public function call(string $action, array $args): bool
    {
        // Return true for acceptance and false for a domain refusal.
        return match ($action) {
$ACTIONS            default => throw new InvalidArgumentException($action),
        };
    }

    // Return any observable subset of $FIELDS.
    public function observe(): array { return []; }
}

final class Zin$MODELTest extends TestCase
{
    private static function plan(): Plan
    {
        static $plan;
        return $plan ??= Plan::load(__DIR__ . '/' . $SPEC);
    }

    public static function cases(): iterable { yield from self::plan()->cases(); }

    #[DataProvider('cases')]
    public function testConforms(int $index): void
    {
        self::plan()->run($index, static fn () => new Zin$MODELHarness());
        $this->addToAssertionCount(1);
    }
}

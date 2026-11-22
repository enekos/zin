<?php declare(strict_types=1);

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Zin\Plan;

require_once __DIR__ . '/../src/Plan.php';

final class Latch
{
    private bool $locked = false;

    public function __construct(private bool $broken = false) {}

    public function call(string $action, array $args): bool
    {
        $target = $action === 'lock';
        if ($this->locked === $target) {
            return $this->broken;
        }
        $this->locked = $target;
        return true;
    }

    public function observe(): array { return ['locked' => $this->locked]; }
}

final class LatchTest extends TestCase
{
    private static function plan(): Plan
    {
        static $plan;
        return $plan ??= Plan::load(__DIR__ . '/../../fixtures/latch.zin');
    }

    public static function cases(): iterable { yield from self::plan()->cases(); }

    #[DataProvider('cases')]
    public function testConformance(int $index): void
    {
        self::plan()->run($index, static fn () => new Latch());
        $this->addToAssertionCount(1);
    }

    public function testBrokenRefusal(): void
    {
        $this->expectException(RuntimeException::class);
        $this->expectExceptionMessage('reject lock');
        self::plan()->run(0, static fn () => new Latch(true));
    }
}

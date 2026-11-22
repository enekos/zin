<?php declare(strict_types=1);

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Zin\Plan;

require_once __DIR__ . '/../src/Plan.php';

final class ObservationsTest extends TestCase
{
    public static function observations(): iterable
    {
        $text = file_get_contents(__DIR__ . '/../../../tests/fixtures/observations.json');
        foreach (json_decode($text, false, 512, JSON_THROW_ON_ERROR) as $case) {
            yield $case->name => [$case];
        }
    }

    #[DataProvider('observations')]
    public function testObservation(stdClass $case): void
    {
        $path = sys_get_temp_dir() . '/zin-observation-' . bin2hex(random_bytes(16)) . '.json';
        try {
            file_put_contents($path, json_encode([
                'zin' => 'plan/1', 'fingerprint' => '0000000000000000', 'parents' => [null], 'system' => 'Observations', 'actions' => [], 'calls' => [],
                'states' => [$case->expected], 'tests' => [['name' => 'initial observation', 'steps' => []]],
            ], JSON_THROW_ON_ERROR));
            $plan = Plan::load($path);
            if (!$case->matches) {
                $this->expectException(RuntimeException::class);
                $this->expectExceptionMessage('step 0');
            }
            $this->assertSame(0, $plan->run(0, static fn () => new class($case->observed) {
                public function __construct(private readonly stdClass $state) {}
                public function observe(): stdClass { return $this->state; }
            }));
        } finally {
            if (file_exists($path)) unlink($path);
        }
    }
}

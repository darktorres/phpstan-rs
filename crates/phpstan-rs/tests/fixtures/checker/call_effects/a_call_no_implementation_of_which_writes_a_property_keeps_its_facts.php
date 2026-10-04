<?php
namespace CallEffects\Logger;

use function PHPStan\Testing\assertType;

interface Logger
{
    public function info(string $message): void;
}

final class BufferLogger implements Logger
{
    /** @var list<string> */
    public array $lines = [];

    public function info(string $message): void
    {
        $this->lines[] = $message;
    }
}

final class Job
{
    public ?int $pending = null;

    public function __construct(private Logger $logger)
    {
    }

    public function step(): int
    {
        if ($this->pending === null) {
            return 0;
        }
        $this->logger->info('step');
        assertType('int', $this->pending);
        return $this->pending;
    }
}

$job = new Job(new BufferLogger());
$job->pending = 3;
echo $job->step();

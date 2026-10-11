<?php
// The canary: classes whose answers are known, run by a worker before it takes
// a unit. A worker that answers differently is not used, so a change to the
// sandbox, the limits or the protocol shows before any table does.

/** Keeps the callable it is given, and runs it when asked. */
final class CanaryKeeper
{
    private ?Closure $held = null;

    public function keep(callable $call): void
    {
        $this->held = Closure::fromCallable($call);
    }

    public function run(): void
    {
        ($this->held)();
    }

    public function ignore(callable $call): void
    {
    }

    public function hang(): void
    {
        for (;;) {
        }
    }

    public function dies(): void
    {
        exit(0);
    }
}

/** Refuses to be made. */
final class CanaryBroken
{
    public function __construct()
    {
        throw new RuntimeException('no object');
    }

    public function keep(callable $call): void
    {
    }

    public function run(): void
    {
    }
}

/**
 * What the canary's units answer that they should not: an empty list when the
 * answers are right.
 *
 * @return list<string>
 */
function canary_problems(): array
{
    $wanted = static fn (string $builtin): bool => str_starts_with($builtin, 'canarykeeper::') || str_starts_with($builtin, 'canarybroken::');
    $kept = run_unit(['held', 'CanaryKeeper', 'keep', 0, 0], $wanted, 'off', []);
    $broken = run_unit(['held', 'CanaryBroken', 'keep', 0, 0], $wanted, 'off', []);
    $expected = [
        "canarykeeper::keep\t\tprobed\t" => true,
        "canarykeeper::run\t\tprobed\t" => true,
        "canarykeeper::run\tby:keep:0\tcalls\t" => true,
        "canarykeeper::ignore\t\tprobed\t" => true,
    ];
    $got = array_keys($kept);
    $want = array_keys($expected);
    sort($got, SORT_STRING);
    sort($want, SORT_STRING);
    $problems = [];
    if ($got !== $want) {
        $problems[] = 'the keeper gave ' . json_encode($got);
    }
    if ($broken !== []) {
        $problems[] = 'the broken class gave ' . json_encode(array_keys($broken));
    }
    return $problems;
}

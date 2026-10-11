<?php
// User classes with known behaviour, probed by the recorder's own units in the
// tests (user classes are never probed by a recording: `probed_classes()` takes
// the internal ones only).

/** Keeps the callable it is given, and runs it when asked. */
final class FixtureStore
{
    private ?Closure $held = null;

    public function store(callable $call): void
    {
        $this->held = Closure::fromCallable($call);
    }

    public function call(): void
    {
        ($this->held)();
    }

    /** Takes the callable and keeps none of it. */
    public function drop(callable $call): void
    {
    }
}

/** Refuses to be made. */
final class FixtureBroken
{
    public function __construct()
    {
        throw new RuntimeException('no object');
    }

    public function store(callable $call): void
    {
    }
}

/** Never returns from its first method. */
final class FixtureHang
{
    public function hang(callable $call): void
    {
        for (;;) {
        }
    }

    public function after(): void
    {
    }
}

/** Ends the process from a method. */
final class FixtureExit
{
    public function exits(): void
    {
        exit(0);
    }
}

/** Ignores the alarm, and leaves a process of its own behind it, whose id goes to FIXTURE_PIDFILE. */
final class FixtureStubborn
{
    public function stubborn(): void
    {
        pcntl_signal(SIGALRM, SIG_IGN);
        exec('sleep 100 >/dev/null 2>&1 & echo $! > ' . escapeshellarg(FIXTURE_PIDFILE));
        for (;;) {
        }
    }
}

<?php

if (($argv[1] ?? null) === '--fatal-child') {
    $events = [];
    register_shutdown_function(static function (): void {
        $error = error_get_last();
        echo 'shutdown:' . ($error['message'] ?? 'none') . "\n";
    });
    $value = new class($events) implements Stringable {
        public function __construct(private array &$events) {}

        public function __toString(): string
        {
            $this->events[] = 'fatal:cast';
            trigger_error('strlen-fatal-probe', E_USER_ERROR);
            $this->events[] = 'fatal:after-error';
            return 'x';
        }
    };
    try {
        strlen($value);
    } finally {
        echo "fatal:finally\n";
    }
    echo "fatal:after-call\n";
    exit(0);
}

/** @return array<string, mixed> */
function run_suspending_strlen(string $name, bool $dynamic): array
{
    $events = [];
    $value = new class($name, $events) implements Stringable {
        public function __construct(
            private string $name,
            private array &$events,
        ) {}

        public function __toString(): string
        {
            $this->events[] = $this->name . ':cast-before-suspend';
            $resumed = Fiber::suspend($this->name . ':suspended');
            $this->events[] = $this->name . ':cast-after-resume:' . $resumed;
            return 'x';
        }
    };
    $callable = 'strlen';
    $fiber = new Fiber(static function () use ($value, $callable, $dynamic, &$events): int {
        $events[] = 'fiber:before:' . ($dynamic ? 'dynamic' : 'direct');
        $length = $dynamic ? $callable($value) : strlen($value);
        $events[] = 'fiber:after:' . ($dynamic ? 'dynamic' : 'direct') . ':' . $length;
        return $length;
    });

    $suspended = $fiber->start();
    $events[] = 'outside:suspended:' . $suspended;
    $events[] = 'outside:is-suspended:' . ($fiber->isSuspended() ? 'yes' : 'no');
    $fiber->resume('resume-value');
    $events[] = 'outside:is-terminated:' . ($fiber->isTerminated() ? 'yes' : 'no');
    $events[] = 'outside:return:' . $fiber->getReturn();
    unset($value);

    return $events;
}

function run_throwing_strlen(bool $dynamic, array &$events): void
{
    $callable = 'strlen';
    try {
        if ($dynamic) {
            $callable(throwing_strlen_value($events));
        } else {
            strlen(throwing_strlen_value($events));
        }
        $events[] = 'throw:after-call';
    } catch (RuntimeException $error) {
        $events[] = 'throw:caught:' . $error->getMessage();
    } finally {
        $events[] = 'throw:finally';
    }
}

function throwing_strlen_value(array &$events): Stringable
{
    return new class($events) implements Stringable {
        public function __construct(private array &$events) {}

        public function __toString(): string
        {
            $this->events[] = 'throw:cast';
            throw new RuntimeException('strlen-throw-probe');
        }

        public function __destruct()
        {
            $this->events[] = 'throw:destruct';
        }
    };
}

$fatalChild = proc_open(
    [PHP_BINARY, '-n', __FILE__, '--fatal-child'],
    [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']],
    $pipes,
);
if (!is_resource($fatalChild)) {
    throw new RuntimeException('could not start fatal child');
}
fclose($pipes[0]);
$fatalStdout = stream_get_contents($pipes[1]);
$fatalStderr = stream_get_contents($pipes[2]);
fclose($pipes[1]);
fclose($pipes[2]);
$fatalStatus = proc_close($fatalChild);
$fatalOutput = $fatalStdout . $fatalStderr;
$fatalReported = str_contains($fatalOutput, 'Fatal error: strlen-fatal-probe');
$fatalShutdown = str_contains($fatalOutput, 'shutdown:strlen-fatal-probe');
$fatalContinued = str_contains($fatalOutput, 'fatal:after-call')
    || str_contains($fatalOutput, 'fatal:after-error');

$directThrow = [];
run_throwing_strlen(false, $directThrow);
$dynamicThrow = [];
run_throwing_strlen(true, $dynamicThrow);
$observed = [
    'direct_suspension' => run_suspending_strlen('direct', false),
    'dynamic_suspension' => run_suspending_strlen('dynamic', true),
    'direct_throw' => $directThrow,
    'dynamic_throw' => $dynamicThrow,
    'fatal_child_status' => $fatalStatus,
    'fatal_reported' => $fatalReported,
    'fatal_shutdown' => $fatalShutdown,
    'fatal_continued' => $fatalContinued,
    'fatal_finally' => str_contains($fatalOutput, 'fatal:finally'),
];
$expected = [
    'direct_suspension' => [
        'fiber:before:direct',
        'direct:cast-before-suspend',
        'outside:suspended:direct:suspended',
        'outside:is-suspended:yes',
        'direct:cast-after-resume:resume-value',
        'fiber:after:direct:1',
        'outside:is-terminated:yes',
        'outside:return:1',
    ],
    'dynamic_suspension' => [
        'fiber:before:dynamic',
        'dynamic:cast-before-suspend',
        'outside:suspended:dynamic:suspended',
        'outside:is-suspended:yes',
        'dynamic:cast-after-resume:resume-value',
        'fiber:after:dynamic:1',
        'outside:is-terminated:yes',
        'outside:return:1',
    ],
    'direct_throw' => [
        'throw:cast',
        'throw:destruct',
        'throw:caught:strlen-throw-probe',
        'throw:finally',
    ],
    'dynamic_throw' => [
        'throw:cast',
        'throw:caught:strlen-throw-probe',
        'throw:finally',
        'throw:destruct',
    ],
    'fatal_child_status' => 255,
    'fatal_reported' => true,
    'fatal_shutdown' => true,
    'fatal_continued' => false,
    'fatal_finally' => false,
];
if ($observed !== $expected) {
    throw new RuntimeException(json_encode($observed, JSON_THROW_ON_ERROR));
}
echo json_encode($observed, JSON_THROW_ON_ERROR), "\n";

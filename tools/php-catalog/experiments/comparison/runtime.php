<?php
// Positive witnesses and precision sentinels. Passing never proves absence.
$passed = [];
function ensure(bool $condition, string $name): void {
    global $passed;
    if (!$condition) throw new RuntimeException($name);
    $passed[] = $name;
}
$calls = 0;
ensure(array_map(function ($x) use (&$calls) { ++$calls; return $x + 1; }, []) === [] && $calls === 0, 'empty-map');
ensure(array_map(null, [1, 2]) === [1, 2], 'null-map');
ensure(array_map(function ($x) use (&$calls) { ++$calls; return $x + 1; }, [1, 2]) === [2, 3] && $calls === 2, 'callback-result-map');
$state = [];
try {
    array_map(function ($x) use (&$state) { $state[] = 'write'; throw new RuntimeException('callback'); }, [1]);
    $state[] = 'after';
} catch (RuntimeException $e) { $state[] = 'catch'; }
ensure($state === ['write', 'catch'], 'write-before-throw');
final class Counted implements Countable {
    public function count(): int { return 7; }
}
ensure(count(new Counted()) === 7, 'countable-dispatch');
$a = [1];
array_walk($a, function (&$v) { $v = 9; });
ensure($a === [9], 'by-reference-walk');
ensure(strlen('abc') === 3, 'scalar-return');
$copy = [1]; $other = $copy; $other[0] = 2;
ensure($copy === [1], 'array-copy');
$cell = 1; $copy = [&$cell]; $other = $copy; $other[0] = 2;
ensure($cell === 2 && $copy === [2], 'shared-reference-cell');
$normalCell = 0; $normalSource = []; $normalSource['value'] =& $normalCell;
$normalCopy = $normalSource;
$normalResult = (function (&$value): int { $value = 3; return 7; })($normalCopy['value']);
ensure($normalResult === 7 && $normalCell === 3 && $normalCopy['value'] === 3, 'array-ref-copy-normal-callback');
$throwCell = 0; $throwSource = []; $throwSource['value'] =& $throwCell;
$throwCopy = $throwSource; $cleanupValue = null;
try {
    try {
        (function (&$value): void { $value = 4; throw new RuntimeException('callback'); })($throwCopy['value']);
    } finally {
        $cleanupValue = $throwCopy['value'];
    }
} catch (RuntimeException $e) {}
ensure($throwCell === 4 && $throwCopy['value'] === 4 && $cleanupValue === 4, 'array-ref-copy-throw-cleanup');
$coercions = 0;
$stringable = new class($coercions) implements Stringable {
    private int $calls;
    public function __construct(int &$calls) { $this->calls =& $calls; }
    public function __toString(): string { ++$this->calls; return 'abc'; }
};
ensure(strlen($stringable) === 3 && $coercions === 1, 'string-coercion-callback');
final class ProbeStream {
    public $context;
    public static array $events = [];
    private bool $read = false;
    public function stream_open(string $path, string $mode, int $options, ?string &$opened): bool {
        self::$events[] = 'open'; return true;
    }
    public function stream_read(int $count): string { self::$events[] = 'read'; $this->read = true; return 'x'; }
    public function stream_eof(): bool { return $this->read; }
    public function stream_close(): void { self::$events[] = 'close'; }
}
stream_wrapper_register('probe', ProbeStream::class);
$stream = fopen('probe://example', 'r');
ensure(fread($stream, 1) === 'x', 'stream-read-result');
fclose($stream);
ensure(ProbeStream::$events === ['open', 'read', 'close'], 'stream-wrapper-dispatch');
if (extension_loaded('session')) {
    $events = [];
    $handler = new class($events) implements SessionHandlerInterface {
        public array $events;
        public function __construct(array &$events) { $this->events =& $events; }
        public function open(string $path, string $name): bool { $this->events[] = 'open'; return true; }
        public function close(): bool { $this->events[] = 'close'; return true; }
        public function read(string $id): string|false { $this->events[] = 'read'; return ''; }
        public function write(string $id, string $data): bool { $this->events[] = 'write'; return true; }
        public function destroy(string $id): bool { return true; }
        public function gc(int $max): int|false { return 0; }
    };
    session_set_save_handler($handler);
    session_start(['use_cookies' => false, 'cache_limiter' => '']);
    session_write_close();
    ensure($events === ['open', 'read', 'write', 'close'], 'retained-session-callbacks');
}
echo implode("\n", $passed), "\n";

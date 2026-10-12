<?php
// What the probes saw, and the objects, streams and callables they are given.

// What the probes saw, in the child: one entry per line of output.
final class Seen
{
    /** @var array<string, true> */
    public static array $entries = [];
    public static string $where = '';
    /** The streams of the recording wrapper closed in this process. */
    public static int $closed = 0;
    /** What callables passed as plain values were called with. */
    public static array $given = [];
    // Set once the call returns: what runs after it (a destructor, a stream
    // closed as it goes) is no part of it.
    public static bool $done = false;
    // The stream context of the context probe, and the URL of the listener it
    // reads from: the first string argument of that call is the URL.
    public static mixed $probeContext = null;
    public static ?string $probeUrl = null;

    public static function add(string $kind, string $detail = ''): void
    {
        if (!self::$done) {
            self::$entries[self::$where . "\t" . $kind . "\t" . $detail] = true;
        }
    }

    public static function returned(): void
    {
        self::add('returned');
        self::$done = true;
    }

    // Recorded after the call, once what the call was given has been let go:
    // the call kept none of it.
    public static function released(string $where): void
    {
        self::$entries[$where . "\t" . 'releases' . "\t"] = true;
    }
}

/** The stream-wrapper methods, each recorded as it runs. */
final class RecordingWrapper
{
    /** @var resource|null */
    public $context;
    private int $read = 0;
    private bool $listed = false;

    public function dir_closedir(): bool { Seen::add('wrapper', 'dir_closedir'); return true; }
    public function dir_opendir(string $path, int $options): bool { Seen::add('wrapper', 'dir_opendir'); return true; }
    public function dir_readdir(): string|false
    {
        Seen::add('wrapper', 'dir_readdir');
        if ($this->listed) {
            return false;
        }
        $this->listed = true;
        return 'entry';
    }
    public function dir_rewinddir(): bool { Seen::add('wrapper', 'dir_rewinddir'); return true; }
    public function mkdir(string $path, int $mode, int $options): bool { Seen::add('wrapper', 'mkdir'); return true; }
    public function rename(string $from, string $to): bool { Seen::add('wrapper', 'rename'); return true; }
    public function rmdir(string $path, int $options): bool { Seen::add('wrapper', 'rmdir'); return true; }
    /** @return resource|false */
    public function stream_cast(int $as) { Seen::add('wrapper', 'stream_cast'); return false; }
    public function stream_close(): void { Seen::$closed++; Seen::add('wrapper', 'stream_close'); }
    public function stream_eof(): bool { Seen::add('wrapper', 'stream_eof'); return $this->read > 0; }
    public function stream_flush(): bool { Seen::add('wrapper', 'stream_flush'); return true; }
    public function stream_lock(int $operation): bool { Seen::add('wrapper', 'stream_lock'); return true; }
    public function stream_metadata(string $path, int $option, mixed $value): bool { Seen::add('wrapper', 'stream_metadata'); return true; }
    public function stream_open(string $path, string $mode, int $options, ?string &$opened): bool { Seen::add('wrapper', 'stream_open'); return true; }
    public function stream_read(int $count): string|false
    {
        Seen::add('wrapper', 'stream_read');
        if ($this->read++ > 0) {
            return '';
        }
        return "a,b\n";
    }
    public function stream_seek(int $offset, int $whence): bool { Seen::add('wrapper', 'stream_seek'); return true; }
    public function stream_set_option(int $option, ?int $arg1, ?int $arg2): bool { Seen::add('wrapper', 'stream_set_option'); return false; }
    /** @return array<int|string, int> */
    public function stream_stat(): array { Seen::add('wrapper', 'stream_stat'); return self::stat(); }
    public function stream_tell(): int { Seen::add('wrapper', 'stream_tell'); return 0; }
    public function stream_truncate(int $size): bool { Seen::add('wrapper', 'stream_truncate'); return true; }
    public function stream_write(string $data): int { Seen::add('wrapper', 'stream_write'); return strlen($data); }
    public function unlink(string $path): bool { Seen::add('wrapper', 'unlink'); return true; }
    /** @return array<int|string, int> */
    public function url_stat(string $path, int $flags): array { Seen::add('wrapper', 'url_stat'); return self::stat(); }

    /** @return array<int|string, int> */
    private static function stat(): array
    {
        $stat = ['dev' => 0, 'ino' => 0, 'mode' => 0100644, 'nlink' => 1, 'uid' => 0, 'gid' => 0,
            'rdev' => 0, 'size' => 4, 'atime' => 0, 'mtime' => 0, 'ctime' => 0, 'blksize' => -1, 'blocks' => -1];
        return array_merge(array_values($stat), $stat);
    }
}

/** An iterator whose methods are recorded. */
final class RecordingIterator implements Iterator
{
    private int $at = 0;
    public function current(): mixed { Seen::add('method', 'current'); return 'value'; }
    public function key(): mixed { Seen::add('method', 'key'); return $this->at; }
    public function next(): void { Seen::add('method', 'next'); $this->at++; }
    public function rewind(): void { Seen::add('method', 'rewind'); $this->at = 0; }
    public function valid(): bool { Seen::add('method', 'valid'); return $this->at < 1; }
}

/** An object whose every method PHP may call is recorded. */
final class RecordingObject implements Countable, ArrayAccess, JsonSerializable, IteratorAggregate, Stringable
{
    public $declared = 1;

    public function count(): int { Seen::add('method', 'count'); return 1; }
    public function offsetExists(mixed $offset): bool { Seen::add('method', 'offsetexists'); return true; }
    public function offsetGet(mixed $offset): mixed { Seen::add('method', 'offsetget'); return 'value'; }
    public function offsetSet(mixed $offset, mixed $value): void { Seen::add('method', 'offsetset'); }
    public function offsetUnset(mixed $offset): void { Seen::add('method', 'offsetunset'); }
    public function jsonSerialize(): mixed { Seen::add('method', 'jsonserialize'); return 1; }
    public function getIterator(): Iterator { Seen::add('method', 'getiterator'); return new RecordingIterator(); }
    public function __toString(): string { Seen::add('method', '__tostring'); return 'string'; }
    public function __get(string $name): mixed { Seen::add('method', '__get'); return null; }
    public function __set(string $name, mixed $value): void { Seen::add('method', '__set'); }
    public function __isset(string $name): bool { Seen::add('method', '__isset'); return false; }
    public function __unset(string $name): void { Seen::add('method', '__unset'); }
    public function __call(string $name, array $args): mixed { Seen::add('method', '__call'); return null; }
    public static function __callStatic(string $name, array $args): mixed { Seen::add('method', '__callstatic'); return null; }
    public function __invoke(mixed ...$args): mixed { Seen::add('method', '__invoke'); return 0; }
    public function __clone(): void { Seen::add('method', '__clone'); }
    /** @return array<string, mixed> */
    public function __debugInfo(): array { Seen::add('method', '__debuginfo'); return []; }
    /** @return array<string, mixed> */
    public function __serialize(): array { Seen::add('method', '__serialize'); return []; }
    /** @param array<string, mixed> $data */
    public function __unserialize(array $data): void { Seen::add('method', '__unserialize'); }
    public function __wakeup(): void { Seen::add('method', '__wakeup'); }
    /** @param array<string, mixed> $state */
    public static function __set_state(array $state): object { Seen::add('method', '__set_state'); return new self(); }
}

/** An object serialized by `__sleep`. */
final class RecordingSleeper
{
    public $declared = 1;
    /** @return list<string> */
    public function __sleep(): array { Seen::add('method', '__sleep'); return ['declared']; }
    public function __wakeup(): void { Seen::add('method', '__wakeup'); }
}

stream_wrapper_register('rec', RecordingWrapper::class);

/** A callable that records being called. */
function recording_callable(): Closure
{
    return static function (mixed ...$args): mixed {
        Seen::add('calls');
        return 0;
    };
}

/** A stream through the recording wrapper; what opening it ran is not recorded. */
function recording_stream()
{
    $saved = Seen::$entries;
    $stream = fopen('rec://stream', 'r+');
    Seen::$entries = $saved;
    return $stream;
}

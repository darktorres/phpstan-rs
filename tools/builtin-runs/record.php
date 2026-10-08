<?php
// Records the user code each builtin runs while it is called: for every
// function and every method of every class PHP defines, which callables
// passed to it it invokes, which methods it calls on an object passed to
// it, which stream-wrapper methods it calls through a URL or a stream
// passed to it, and, for a method, which code its object was given when it
// was made that it runs. What a call shows is what it ran with the values
// given; a builtin can run more with others. Prints one line for each,
// tab-separated:
//
//   <builtin> <where> <kind> <detail>
//
// <builtin> is a function's name, or `Class::method`, lowercased. <where>
// is the parameter's position (from 0), followed by `[]` when what ran was
// reached through the elements of an array passed there, and prefixed by
// `new:` for a parameter of the class's constructor, or by `by:<method>:`
// for a parameter of another method of the class, called on the object
// before this one. <kind> is `calls` (the callable
// was invoked), `method` (the method <detail> was called on the object),
// `wrapper` (the wrapper method <detail> ran), or `releases` (a function
// given a callable at a parameter declared to take one kept none of it,
// nor of what it gave back, once it returned; a position with no such line
// may keep it). Every builtin that was
// called is listed with the kind `probed`; a builtin missing from the
// output is one this PHP does not define, or one no call reached (an
// abstract class's method, a constructor that would not take the values).
//
// Each call runs in a child process of its own, in an empty directory, with
// an alarm, so that a call that ends the process, changes its state or
// blocks affects no other. Run it with the PHP whose behaviour is wanted:
//
//   php tools/builtin-runs/record.php > crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv
//
// A regular expression as the first argument limits it to the builtins
// whose lowercased name it matches. `--jobs=N` sets the number of workers
// (one for each processor by default): each takes the next function or
// method from a queue as it finishes its last, so the machine stays busy
// to the end.

declare(strict_types=0);

error_reporting(0);
ini_set('display_errors', '0');
ini_set('memory_limit', '512M');

if (!function_exists('pcntl_fork')) {
    fwrite(STDERR, "pcntl is needed\n");
    exit(1);
}

// A probe that aborts (a failed assertion in an extension, a fault) must not
// start the system's crash handler, which costs seconds for each: processes
// that are not dumpable never start it, and the setting is inherited by every
// worker and probe. It needs FFI, which PHP allows only when asked.
if (extension_loaded('ffi')) {
    if (ini_get('ffi.enable') !== '1' && getenv('BUILTIN_RUNS_REEXEC') === false) {
        putenv('BUILTIN_RUNS_REEXEC=1');
        pcntl_exec(PHP_BINARY, ['-d', 'ffi.enable=1', ...$argv]);
    }
    try {
        FFI::cdef('int prctl(int option, unsigned long a, unsigned long b, unsigned long c, unsigned long d);', 'libc.so.6')
            ->prctl(4, 0, 0, 0, 0);
    } catch (Throwable) {
    }
}

// What the probes saw, in the child: one entry per line of output.
final class Seen
{
    /** @var array<string, true> */
    public static array $entries = [];
    public static string $where = '';
    /** What callables passed as plain values were called with. */
    public static array $given = [];
    // Set once the call returns: what runs after it (a destructor, a stream
    // closed as it goes) is no part of it.
    public static bool $done = false;

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
    public function stream_close(): void { Seen::add('wrapper', 'stream_close'); }
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
    public function stream_set_option(int $option, int $arg1, int $arg2): bool { Seen::add('wrapper', 'stream_set_option'); return false; }
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

/** Whether `$type` admits a value of the named type. */
function admits(?ReflectionType $type, string $name): bool
{
    if ($type === null) {
        return true;
    }
    if ($type instanceof ReflectionNamedType) {
        $t = strtolower($type->getName());
        return $t === $name || $t === 'mixed'
            || ($name === 'object' && !in_array($t, ['int', 'float', 'string', 'bool', 'array', 'null', 'false', 'true', 'void', 'never', 'callable', 'iterable', 'resource'], true))
            || ($name === 'iterable' && in_array($t, ['iterable', 'traversable', 'iterator', 'iteratoraggregate'], true));
    }
    foreach ($type->getTypes() as $member) {
        if (admits($member, $name)) {
            return true;
        }
    }
    return false;
}

/** Whether `$type` names a callable, not merely admits one as `mixed` does. */
function names_callable(?ReflectionType $type): bool
{
    if ($type instanceof ReflectionNamedType) {
        return in_array(strtolower($type->getName()), ['callable', 'closure'], true);
    }
    if ($type instanceof ReflectionUnionType) {
        foreach ($type->getTypes() as $member) {
            if (names_callable($member)) {
                return true;
            }
        }
    }
    return false;
}

/**
 * The probes for a parameter: each a description and a function that makes
 * the value passed.
 *
 * @return list<array{string, Closure(): mixed}>
 */
function probes_for(ReflectionParameter $param): array
{
    $type = $param->getType();
    $probes = [];
    if (admits($type, 'callable') || admits($type, 'closure')) {
        $probes[] = ['callable', static fn () => recording_callable()];
    }
    if (admits($type, 'object') || admits($type, 'iterable')) {
        $probes[] = ['object', static fn () => new RecordingObject()];
        $probes[] = ['iterator', static fn () => new RecordingIterator()];
        $probes[] = ['sleeper', static fn () => new RecordingSleeper()];
    }
    if (admits($type, 'string')) {
        $probes[] = ['url', static fn () => 'rec://probe'];
    }
    if (admits($type, 'array')) {
        $probes[] = ['objects', static fn () => [new RecordingObject(), new RecordingObject()]];
        $probes[] = ['callables', static fn () => [recording_callable(), recording_callable()]];
        $probes[] = ['urls', static fn () => ['rec://probe']];
    }
    if (admits($type, 'resource') || $type === null || (string) $type === 'mixed') {
        $probes[] = ['stream', static fn () => recording_stream()];
    }
    return $probes;
}

/** A plain value for a parameter a probe is not at. */
function plain_value(ReflectionParameter $param): mixed
{
    $type = $param->getType();
    if ($type === null) {
        return 1;
    }
    if ($type->allowsNull() && !admits($type, 'int') && !admits($type, 'string')) {
        return null;
    }
    foreach (['int' => 1, 'string' => 'x', 'float' => 1.0, 'bool' => false, 'array' => [1, 2]] as $name => $value) {
        if (admits($type, $name)) {
            return $value;
        }
    }
    if (admits($type, 'callable')) {
        return static function (mixed ...$args): int {
            Seen::$given[] = $args;
            return 0;
        };
    }
    if (admits($type, 'iterable')) {
        return [1, 2];
    }
    if (admits($type, 'object')) {
        return new stdClass();
    }
    return null;
}

/**
 * The arguments for a call with the probe value at `$at`: the parameters
 * before it plain, the ones after it left out when they may be.
 *
 * @param list<ReflectionParameter> $params
 * @return list<mixed>
 */
function arguments(array $params, int $at, mixed $probe): array
{
    $args = [];
    foreach ($params as $i => $param) {
        if ($i > $at && $param->isOptional()) {
            break;
        }
        $args[$i] = $i === $at ? $probe : plain_value($param);
    }
    return $args;
}

/**
 * Runs `$call` in a child process and gives back what the probes saw, or
 * `null` when the child did not finish.
 *
 * @return array<string, true>|null
 */
function in_child(Closure $call): ?array
{
    $pair = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);
    $pid = pcntl_fork();
    if ($pid === -1) {
        fwrite(STDERR, "fork failed\n");
        exit(1);
    }
    if ($pid === 0) {
        fclose($pair[0]);
        pcntl_alarm(3);
        // What the child prints would mix with the parent's output.
        ob_start(static fn (string $buffer): string => '');
        try {
            $call();
        } catch (Throwable) {
        }
        @fwrite($pair[1], serialize(Seen::$entries));
        fclose($pair[1]);
        posix_kill(posix_getpid(), SIGKILL);
    }
    fclose($pair[1]);
    $data = stream_get_contents($pair[0]);
    fclose($pair[0]);
    pcntl_waitpid($pid, $status);
    if ($data === '' || $data === false) {
        return null;
    }
    $seen = @unserialize($data, ['allowed_classes' => false]);
    return is_array($seen) ? $seen : null;
}

/** Where a probe is: its parameter's position, and `[]` for the elements of an array. */
function where(string $prefix, int $at, string $probe): string
{
    return $prefix . $at . (in_array($probe, ['objects', 'callables', 'urls'], true) ? '[]' : '');
}

/** The lines for one probe's call: `probed` once the call was reached. */
function record(string $builtin, string $where, ?array $seen, array &$out): void
{
    if ($seen === null) {
        return;
    }
    foreach (array_keys($seen) as $entry) {
        [$seenWhere, $kind, $detail] = explode("\t", $entry);
        if ($kind === 'reached') {
            $out["$builtin\t\tprobed\t"] = true;
            continue;
        }
        if ($seenWhere !== $where || $kind === 'returned') {
            continue;
        }
        $out["$builtin\t$where\t$kind\t$detail"] = true;
    }
}

// Calls that would stop the probing itself or reach outside the child.
const SKIPPED = [
    'pcntl_fork', 'pcntl_rfork', 'pcntl_exec', 'pcntl_alarm', 'pcntl_signal', 'pcntl_wait', 'pcntl_waitpid',
    'posix_kill', 'posix_setsid', 'posix_setuid', 'posix_setgid', 'posix_seteuid', 'posix_setegid',
    'exec', 'system', 'passthru', 'shell_exec', 'proc_open', 'popen',
    'sleep', 'usleep', 'time_nanosleep', 'time_sleep_until',
    'readline', 'readline_callback_handler_install',
    'mail', 'dl', 'stream_socket_server', 'socket_create_listen', 'fsockopen', 'pfsockopen',
    'stream_socket_client', 'socket_connect', 'curl_exec', 'curl_multi_exec', 'ftp_connect', 'ftp_ssl_connect',
    'mysqli_connect', 'mysqli_real_connect', 'pg_connect', 'pg_pconnect', 'gethostbyname', 'gethostbynamel',
    'dns_get_record', 'checkdnsrr', 'getmxrr', 'gethostbyaddr', 'get_headers', 'stream_wrapper_unregister',
    'stream_wrapper_restore', 'excimer_set_timeout',
];

// Functions the probing skips because they reach outside the child (a
// program, the network, a delay) but take no callable and no object whose
// methods PHP calls: they run no user code, as a call of them is taken.
const RUNS_NOTHING = [
    'exec', 'system', 'passthru', 'shell_exec', 'proc_open', 'popen',
    'sleep', 'usleep', 'time_nanosleep', 'time_sleep_until',
    'mail', 'fsockopen', 'pfsockopen', 'stream_socket_server',
    'gethostbyname', 'gethostbynamel', 'gethostbyaddr', 'dns_get_record', 'checkdnsrr', 'getmxrr',
];

/** The classes whose methods are probed: the ones PHP defines, not the recorders. */
function probed_classes(): array
{
    $classes = [];
    foreach (get_declared_classes() as $class) {
        $reflection = new ReflectionClass($class);
        if ($reflection->isInternal() && !in_array($class, [RecordingWrapper::class, RecordingIterator::class, RecordingObject::class, RecordingSleeper::class, Seen::class], true)) {
            $classes[] = $class;
        }
    }
    return $classes;
}

/**
 * How to make an object of `$reflection` to call a method on: plainly, and
 * with a probe at each constructor parameter.
 *
 * @return list<array{string, Closure(): mixed}>
 */
function makers(ReflectionClass $reflection): array
{
    $constructor = $reflection->getConstructor();
    $ctorParams = $constructor?->getParameters() ?? [];
    $makers = [['', static fn () => $reflection->isInstantiable()
        ? ($constructor === null ? $reflection->newInstance() : $reflection->newInstanceArgs(arguments($ctorParams, -1, null)))
        : null]];
    foreach ($ctorParams as $at => $param) {
        foreach (probes_for($param) as [$probe, $make]) {
            $makers[] = [where('new:', $at, $probe), static fn () => $reflection->newInstanceArgs(arguments($ctorParams, $at, $make()))];
        }
    }
    return $makers;
}

/** The public methods a class declares itself, not abstract. */
function own_methods(ReflectionClass $reflection): array
{
    return array_values(array_filter(
        $reflection->getMethods(ReflectionMethod::IS_PUBLIC),
        static fn (ReflectionMethod $method): bool => !$method->isAbstract() && $method->getDeclaringClass()->getName() === $reflection->getName(),
    ));
}

/** The lines for a function. */
function probe_function(string $function, array &$out): void
{
    $reflection = new ReflectionFunction($function);
    $params = $reflection->getParameters();
    $seen = in_child(static function () use ($function, $params): void {
        Seen::$where = 'plain';
        Seen::add('reached');
        $function(...arguments($params, -1, null));
        Seen::returned();
    });
    record(strtolower($function), 'plain', $seen, $out);
    foreach ($params as $at => $param) {
        foreach (probes_for($param) as [$probe, $make]) {
            $where = where('', $at, $probe);
            $seen = in_child(static function () use ($function, $params, $at, $make, $where, $probe): void {
                $weak = null;
                // What the call gave back and the other arguments, which a
                // caller still holds once it returned.
                $kept = (static function () use ($function, $params, $at, $make, $where, &$weak): array {
                    $value = $make();
                    if (is_object($value)) {
                        $weak = WeakReference::create($value);
                    }
                    Seen::$entries = [];
                    Seen::$where = $where;
                    $args = arguments($params, $at, $value);
                    $refs = [];
                    foreach ($args as $i => $arg) {
                        $refs[$i] = $arg;
                        $args[$i] = &$refs[$i];
                    }
                    Seen::add('reached');
                    $result = call_user_func_array($function, $args);
                    Seen::returned();
                    unset($refs[$at]);
                    return [$result, $refs];
                })();
                if ($probe === 'callable' && $weak !== null && names_callable($params[$at]->getType()) && !$params[$at]->isPassedByReference()) {
                    gc_collect_cycles();
                    if ($weak->get() === null) {
                        Seen::released($where);
                    }
                }
                unset($kept);
            });
            record(strtolower($function), $where, $seen, $out);
        }
    }
}

/** The lines for a method: plainly, with a probe at each parameter, and on an object made with a probe. */
function probe_method(string $class, string $name, array &$out): void
{
    $reflection = new ReflectionClass($class);
    $method = $reflection->getMethod($name);
    $makers = makers($reflection);
    $builtin = strtolower($class . '::' . $name);
    $params = $method->getParameters();
    $static = $method->isStatic();
    // The method, plainly, on a plain object.
    $seen = in_child(static function () use ($makers, $static, $class, $name, $params): void {
        $object = $static ? null : ($makers[0][1])();
        if (!$static && $object === null) {
            return;
        }
        Seen::$entries = [];
        Seen::$where = 'plain';
        Seen::add('reached');
        $args = arguments($params, -1, null);
        $static ? $class::$name(...$args) : $object->$name(...$args);
        Seen::returned();
    });
    record($builtin, 'plain', $seen, $out);
    // A probe at each of the method's parameters, on a plain object.
    foreach ($params as $at => $param) {
        foreach (probes_for($param) as [$probe, $make]) {
            $where = where('', $at, $probe);
            $seen = in_child(static function () use ($makers, $static, $class, $name, $params, $at, $make, $where): void {
                $object = $static ? null : ($makers[0][1])();
                if (!$static && $object === null) {
                    return;
                }
                $value = $make();
                Seen::$entries = [];
                Seen::$where = $where;
                $args = arguments($params, $at, $value);
                Seen::add('reached');
                $static ? $class::$name(...$args) : $object->$name(...$args);
                Seen::returned();
            });
            record($builtin, $where, $seen, $out);
        }
    }
    if ($static) {
        return;
    }
    // The method, plainly, on an object made with a probe.
    foreach (array_slice($makers, 1) as [$where, $make]) {
        $seen = in_child(static function () use ($make, $where, $name, $params): void {
            Seen::$where = $where;
            $object = $make();
            Seen::$entries = [];
            Seen::add('reached');
            $object->$name(...arguments($params, -1, null));
            Seen::returned();
        });
        record($builtin, $where, $seen, $out);
    }
}

/**
 * The lines for the methods of a class called after `$firstName` was given
 * the probe `$probeAt` of its parameter `$paramAt`: what each runs of what
 * the first kept.
 */
function probe_held(string $class, string $firstName, int $paramAt, int $probeAt, callable $wanted, array &$out): void
{
    $reflection = new ReflectionClass($class);
    if (!$reflection->isInstantiable()) {
        return;
    }
    $makers = makers($reflection);
    $instance = array_values(array_filter(
        own_methods($reflection),
        static fn (ReflectionMethod $method): bool => !$method->isStatic() && $method->getName() !== '__construct',
    ));
    $firstParams = $reflection->getMethod($firstName)->getParameters();
    foreach ($firstParams as $at => $param) {
        if ($at !== $paramAt) {
            continue;
        }
        foreach (probes_for($param) as $index => [$probe, $make]) {
            if ($index !== $probeAt) {
                continue;
            }
            foreach ($instance as $then) {
                $builtin = strtolower($class . '::' . $then->getName());
                if (!$wanted($builtin)) {
                    continue;
                }
                $where = 'by:' . strtolower($firstName) . ':' . where('', $at, $probe);
                $thenName = $then->getName();
                $thenParams = $then->getParameters();
                $seen = in_child(static function () use ($makers, $firstName, $firstParams, $at, $make, $where, $thenName, $thenParams): void {
                    $object = ($makers[0][1])();
                    if ($object === null) {
                        return;
                    }
                    $value = $make();
                    try {
                        $object->$firstName(...arguments($firstParams, $at, $value));
                    } catch (Throwable) {
                    }
                    Seen::$entries = [];
                    Seen::$where = $where;
                    Seen::add('reached');
                    $object->$thenName(...arguments($thenParams, -1, null));
                    Seen::returned();
                });
                record($builtin, $where, $seen, $out);
            }
        }
    }
}

/**
 * The units of work, each a function, a method, or a method given one probe
 * at one parameter and followed by each other method of the class: small
 * enough, and taken the dearest first, that a pool of workers taking the
 * next one as it frees keeps every worker busy to the end.
 *
 * @return list<array{string, string, string, int, int}>
 */
function units(callable $wanted): array
{
    $costed = [];
    $add = static function (int $cost, array $unit) use (&$costed): void {
        $costed[] = [$cost, count($costed), $unit];
    };
    foreach (get_defined_functions()['internal'] as $function) {
        if (in_array($function, SKIPPED, true) || str_starts_with($function, 'posix_') || str_starts_with($function, 'pcntl_')) {
            continue;
        }
        if ($wanted(strtolower($function))) {
            $add(1, ['function', $function, '', 0, 0]);
        }
    }
    foreach (probed_classes() as $class) {
        $reflection = new ReflectionClass($class);
        $own = own_methods($reflection);
        $instance = array_filter($own, static fn (ReflectionMethod $method): bool => !$method->isStatic() && $method->getName() !== '__construct');
        foreach ($own as $method) {
            if ($wanted(strtolower($class . '::' . $method->getName()))) {
                $add(1 + count($method->getParameters()), ['method', $class, $method->getName(), 0, 0]);
            }
            if (!$reflection->isInstantiable() || $method->isStatic() || $method->getName() === '__construct') {
                continue;
            }
            foreach ($method->getParameters() as $at => $param) {
                foreach (array_keys(probes_for($param)) as $index) {
                    $add(count($instance), ['held', $class, $method->getName(), $at, $index]);
                }
            }
        }
    }
    usort($costed, static fn (array $a, array $b): int => [$b[0], $a[1]] <=> [$a[0], $b[1]]);
    return array_map(static fn (array $entry): array => $entry[2], $costed);
}

/** Runs one unit, giving back the lines it found. */
function run_unit(array $unit, callable $wanted): array
{
    $out = [];
    [$kind, $name, $method, $paramAt, $probeAt] = $unit;
    match ($kind) {
        'function' => probe_function($name, $out),
        'method' => probe_method($name, $method, $out),
        'held' => probe_held($name, $method, $paramAt, $probeAt, $wanted, $out),
    };
    return $out;
}

/** Removes a directory and what is in it. */
function remove_directory(string $dir): void
{
    chdir('/');
    if (!is_dir($dir)) {
        return;
    }
    $entries = new RecursiveIteratorIterator(
        new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS),
        RecursiveIteratorIterator::CHILD_FIRST,
    );
    foreach ($entries as $entry) {
        $entry->isDir() && !$entry->isLink() ? rmdir($entry->getPathname()) : unlink($entry->getPathname());
    }
    rmdir($dir);
}

/** A message on a socket: one line. */
function send(mixed $socket, mixed $message): void
{
    fwrite($socket, base64_encode(serialize($message)) . "\n");
}

/** The message on a socket, or `null` when it was closed. */
function receive(mixed $socket): mixed
{
    $line = fgets($socket);
    if ($line === false) {
        return null;
    }
    return unserialize(base64_decode(trim($line)), ['allowed_classes' => false]);
}

/**
 * A worker: takes units from the parent one at a time, in a directory of
 * its own, and sends back the lines each found.
 */
function work(mixed $socket, callable $wanted): never
{
    $dir = sys_get_temp_dir() . '/builtin-runs-' . getmypid();
    @mkdir($dir);
    chdir($dir);
    send($socket, ['ready', []]);
    while (is_array($unit = receive($socket))) {
        send($socket, ['done', run_unit($unit, $wanted)]);
    }
    remove_directory($dir);
    exit(0);
}

/**
 * Runs the units on `$jobs` workers: each takes the next unit from the front
 * of the queue when it has finished its last.
 *
 * @param list<array{string, string, string, int, int}> $units
 * @return array<string, true>
 */
function run_units(array $units, int $jobs, callable $wanted): array
{
    $out = [];
    $sockets = [];
    for ($i = 0; $i < $jobs; $i++) {
        $pair = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);
        $pid = pcntl_fork();
        if ($pid === -1) {
            fwrite(STDERR, "fork failed\n");
            exit(1);
        }
        if ($pid === 0) {
            fclose($pair[0]);
            foreach ($sockets as $other) {
                fclose($other);
            }
            work($pair[1], $wanted);
        }
        fclose($pair[1]);
        $sockets[$pid] = $pair[0];
    }
    $next = 0;
    $total = count($units);
    while ($sockets !== []) {
        $read = array_values($sockets);
        $write = $except = null;
        if (stream_select($read, $write, $except, null) === false) {
            fwrite(STDERR, "select failed\n");
            exit(1);
        }
        foreach ($read as $socket) {
            $pid = array_search($socket, $sockets, true);
            $message = receive($socket);
            if (is_array($message)) {
                foreach (array_keys($message[1]) as $line) {
                    $out[$line] = true;
                }
            }
            if ($message !== null && $next < $total) {
                send($socket, $units[$next++]);
                if ($next % 500 === 0) {
                    fwrite(STDERR, "$next / $total\n");
                }
                continue;
            }
            fclose($socket);
            unset($sockets[$pid]);
            pcntl_waitpid($pid, $status);
        }
    }
    return $out;
}

/** The number of workers asked for, or one for each processor. */
function jobs(array $argv): int
{
    foreach ($argv as $argument) {
        if (str_starts_with($argument, '--jobs=')) {
            return max(1, (int) substr($argument, 7));
        }
    }
    return max(1, (int) shell_exec('nproc 2>/dev/null') ?: 1);
}

$arguments = array_values(array_filter(array_slice($argv, 1), static fn (string $argument): bool => !str_starts_with($argument, '--')));
$only = $arguments[0] ?? null;
$wanted = static fn (string $builtin): bool => $only === null || preg_match($only, $builtin) === 1;

$out = run_units(units($wanted), jobs($argv), $wanted);

foreach (RUNS_NOTHING as $function) {
    if (function_exists($function) && $wanted($function)) {
        $out["$function\t\tprobed\t"] = true;
    }
}

$lines = array_keys($out);
sort($lines, SORT_STRING);
echo implode("\n", $lines), "\n";

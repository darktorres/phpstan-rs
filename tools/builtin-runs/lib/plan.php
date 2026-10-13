<?php
// What is probed: the skipped names, the classes, and the units of work.

// Calls that would stop the probing itself, or read the terminal. The
// network, process, delay and mail functions are probed, in the sandbox.
const SKIPPED = [
    'pcntl_fork', 'pcntl_rfork', 'pcntl_exec', 'pcntl_alarm', 'pcntl_signal', 'pcntl_wait', 'pcntl_waitpid',
    'posix_kill', 'posix_setsid', 'posix_setuid', 'posix_setgid', 'posix_seteuid', 'posix_setegid',
    'readline', 'readline_callback_handler_install', 'dl',
    'stream_wrapper_unregister', 'stream_wrapper_restore', 'excimer_set_timeout',
];

// Functions asserted to run no user code (a program, the network, a delay, a
// mail) where no call of them was measured: a row is added only for a name
// with no `probed` row.
/**
 * Methods a live server would block on or change: subscriptions and blocking pops wait for a message or an
 * element; the others stop, reconfigure or empty the server, or open and close connections. Never called.
 */
const METHODS_SKIPPED = [
    'redis::subscribe', 'redis::psubscribe', 'redis::ssubscribe', 'redis::monitor', 'redis::blpop', 'redis::brpop',
    'redis::brpoplpush', 'redis::blmove', 'redis::bzpopmin', 'redis::bzpopmax', 'redis::bzmpop', 'redis::blmpop',
    'redis::wait', 'redis::waitaof', 'redis::flushall', 'redis::flushdb', 'redis::shutdown', 'redis::slaveof',
    'redis::replicaof', 'redis::failover', 'redis::bgrewriteaof', 'redis::bgsave', 'redis::save', 'redis::config',
    'redis::debug', 'redis::migrate', 'redis::sync', 'redis::psync', 'redis::client', 'redis::script',
    'redis::function', 'redis::auth', 'redis::select', 'redis::swapdb', 'redis::acl', 'redis::module',
    'redis::cluster', 'redis::reset', 'redis::connect', 'redis::pconnect', 'redis::popen', 'redis::open',
    'redis::close', 'redis::multi', 'redis::exec', 'redis::discard', 'redis::watch', 'redis::unwatch',
    // The serializer and the other options apply to every later call, which an object or a callable as the value breaks.
    'redis::setoption',
    // A MySQL connection made with probe values leaves the object unable to answer the next call.
    'mysqli::connect', 'mysqli::real_connect', 'mysqli::change_user',
];

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
 * A maker for a class whose constructor refuses every generated value, from
 * valid arguments: the reflection classes name a class or a function of the
 * recorder's own (or of PHP), and SplFileObject opens the seeded file `x`, which
 * a child's directory holds. A class without a recipe is made from the generated
 * values, as before.
 */
/**
 * The object a method is called on. A constructor is called on an object it has not yet constructed, so the
 * object is made without one; any other method is called on the maker's object.
 */
function method_receiver(ReflectionClass $reflection, string $method, Closure $maker): ?object
{
    // SQLite3::open opens a database for an object that has none: it is the constructor's other half.
    $unconstructed = $method === '__construct' || ($method === 'open' && strtolower($reflection->getName()) === 'sqlite3');
    return $unconstructed ? $reflection->newInstanceWithoutConstructor() : $maker();
}

function recipe(string $class): ?Closure
{
    return match (strtolower($class)) {
        'reflectionproperty' => static fn (): object => new ReflectionProperty(RecordingObject::class, 'declared'),
        'reflectionmethod' => static fn (): object => new ReflectionMethod(RecordingObject::class, 'count'),
        'reflectionparameter' => static fn (): object => new ReflectionParameter([RecordingObject::class, 'offsetGet'], 0),
        'reflectionclassconstant' => static fn (): object => new ReflectionClassConstant(Attribute::class, 'TARGET_CLASS'),
        'reflectionextension' => static fn (): object => new ReflectionExtension('standard'),
        'reflectionclass' => static fn (): object => new ReflectionClass(stdClass::class),
        'splfileobject' => static fn (): object => new SplFileObject(recipe_file(), 'w+'),
        'splfileinfo' => static fn (): object => new SplFileInfo(recipe_file()),
        // Imagick's methods throw on an empty object: one image is read first.
        'imagick' => static function (): object {
            $image = new Imagick();
            $image->newImage(16, 16, new ImagickPixel('white'));
            return $image;
        },
        'ziparchive' => static function (): object {
            $zip = new ZipArchive();
            $zip->open(recipe_file(), ZipArchive::CREATE);
            $zip->addFromString('a.txt', 'x');
            return $zip;
        },
        'xmlwriter' => static fn (): object => xmlwriter_open_memory(),
        'xmlreader' => static fn (): object => XMLReader::XML('<a>x</a>'),
        'xsltprocessor' => static function (): object {
            $processor = new XSLTProcessor();
            $stylesheet = new DOMDocument();
            $stylesheet->loadXML('<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/"><xsl:copy-of select="."/></xsl:template></xsl:stylesheet>');
            $processor->importStylesheet($stylesheet);
            return $processor;
        },
        'pdostatement' => static fn (): object => (new PDO('sqlite::memory:'))->query('SELECT 1'),
        // A DOM node of a document, and a text node of one.
        'domnode' => static function (): object {
            $document = new DOMDocument();
            $document->loadXML('<a>x</a>');
            return $document->documentElement;
        },
        'domcharacterdata' => static fn (): object => (new DOMDocument())->createTextNode('x'),
        'spldoublylinkedlist' => static function (): object {
            $list = new SplDoublyLinkedList();
            $list->push(1);
            $list->push(2);
            return $list;
        },
        'splfixedarray' => static fn (): object => new SplFixedArray(2),
        'ds\\vector' => static fn (): object => new Ds\Vector([1, 2]),
        'ds\\deque' => static fn (): object => new Ds\Deque([1, 2]),
        'ds\\queue' => static fn (): object => new Ds\Queue([1, 2]),
        'ds\\stack' => static fn (): object => new Ds\Stack([1, 2]),
        'ds\\set' => static fn (): object => new Ds\Set([1, 2]),
        'directoryiterator' => static fn (): object => new DirectoryIterator(sys_get_temp_dir()),
        'filesystemiterator' => static fn (): object => new FilesystemIterator(sys_get_temp_dir()),
        'recursivedirectoryiterator' => static fn (): object => new RecursiveDirectoryIterator(sys_get_temp_dir()),
        'globiterator' => static fn (): object => new GlobIterator(sys_get_temp_dir() . '/*'),
        // Their constructors cannot run twice, so a method unit gets an object made without one.
        'intlgregoriancalendar' => static fn (): object => IntlGregorianCalendar::createInstance('UTC', 'en'),
        'intldatepatterngenerator' => static fn (): object => IntlDatePatternGenerator::create('en'),
        'arrayiterator' => static fn (): object => new ArrayIterator([1, 2]),
        'ds\\map' => static fn (): object => new Ds\Map(['a' => 1, 'b' => 2]),
        'finfo' => static fn (): object => new finfo(FILEINFO_MIME_TYPE),
        'fiber' => static fn (): object => new Fiber(static fn (): int => 1),
        // Connected to the MySQL server the scan starts inside its sandbox, on a Unix socket.
        'mysqli' => static fn (): object => new mysqli(null, 'root', '', '', 0, server_run_dir() . '/mysql.sock'),
        // Connected to the Redis server the scan starts inside its sandbox, on a Unix socket.
        'redis' => static function (): object {
            $redis = new Redis();
            $redis->connect(server_run_dir() . '/redis.sock');
            return $redis;
        },
        'splobjectstorage' => static function (): object {
            $storage = new SplObjectStorage();
            $storage->attach(new stdClass());
            $storage->attach(new stdClass());
            return $storage;
        },
        'imagickkernel' => static fn (): object => ImagickKernel::fromBuiltin(Imagick::KERNEL_DIAMOND, '1'),
        default => null,
    };
}

/** A temporary file of the worker's own /tmp, which the sandbox makes private. */
function recipe_file(): string
{
    $path = tempnam(sys_get_temp_dir(), 'rec');
    return $path;
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
    $plain = recipe($reflection->getName()) ?? static fn () => $reflection->isInstantiable()
        ? ($constructor === null ? $reflection->newInstance() : $reflection->newInstanceArgs(arguments($ctorParams, -1, null)))
        : null;
    $makers = [['', $plain]];
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
            if (in_array(strtolower($class . '::' . $method->getName()), METHODS_SKIPPED, true)) {
                continue;
            }
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

/**
 * The children a unit makes: counted from the reflection of what it calls,
 * as the probes of `probe_function`, `probe_method` and `probe_held` are.
 *
 * @param array{string, string, string, int, int} $unit
 */
function unit_forks(array $unit, callable $wanted): int
{
    [$kind, $name, $method, $paramAt, $probeAt] = $unit;
    if ($kind === 'function') {
        $forks = 1;
        foreach ((new ReflectionFunction($name))->getParameters() as $param) {
            $forks += count(probes_for($param));
        }
        return $forks;
    }
    $reflection = new ReflectionClass($name);
    if ($kind === 'method') {
        $target = $reflection->getMethod($method);
        $forks = 1;
        foreach ($target->getParameters() as $param) {
            $forks += count(probes_for($param));
        }
        return $target->isStatic() ? $forks : $forks + count(makers($reflection)) - 1;
    }
    $forks = 0;
    foreach (own_methods($reflection) as $then) {
        if (!$then->isStatic() && $then->getName() !== '__construct' && $wanted(strtolower($name . '::' . $then->getName()))) {
            $forks++;
        }
    }
    return $forks;
}

/**
 * The children each unit makes.
 *
 * @param list<array{string, string, string, int, int}> $units
 * @return list<int>
 */
function unit_weights(array $units, callable $wanted): array
{
    return array_map(static fn (array $unit): int => unit_forks($unit, $wanted), $units);
}

/**
 * A hash of the declarations a unit calls: its function, its method, or for a
 * held unit the whole class, whose methods the held unit calls.
 *
 * @param array{string, string, string, int, int} $unit
 */
function unit_signature(array $unit): string
{
    [$kind, $name, $method] = $unit;
    if ($kind === 'function') {
        return sha1((string) new ReflectionFunction($name));
    }
    $reflection = new ReflectionClass($name);
    return sha1($kind === 'method' ? (string) $reflection->getMethod($method) : (string) $reflection);
}

/**
 * What a run would do: the units and the children of each kind, and the
 * classes with the most children.
 *
 * @param list<array{string, string, string, int, int}> $units
 * @param list<int> $weights
 */
function plan_summary(array $units, array $weights): string
{
    $count = $forks = $byClass = [];
    foreach ($units as $i => [$kind, $name]) {
        $count[$kind] = ($count[$kind] ?? 0) + 1;
        $forks[$kind] = ($forks[$kind] ?? 0) + $weights[$i];
        $class = $kind === 'function' ? '(functions)' : $name;
        $byClass[$class] = ($byClass[$class] ?? 0) + $weights[$i];
    }
    arsort($byClass);
    $kinds = static fn (array $by): string => implode(', ', array_map(
        static fn (string $kind): string => $kind . ' ' . ($by[$kind] ?? 0),
        ['function', 'method', 'held'],
    ));
    $lines = [
        'units ' . count($units) . ' (' . $kinds($count) . ')',
        'forks ' . array_sum($weights) . ' (' . $kinds($forks) . ')',
        'classes with the most forks:',
    ];
    foreach (array_slice($byClass, 0, 10, true) as $class => $n) {
        $lines[] = sprintf('  %-40s %d', $class, $n);
    }
    return implode("\n", $lines) . "\n";
}

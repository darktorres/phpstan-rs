<?php
// What is probed: the skipped names, the classes, and the units of work.

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

<?php
// Connected MySQL (mysqli) and PostgreSQL (pgsql) facts from scratch servers, on 127.0.0.1:13306 and
// 127.0.0.1:15432 (see README.md). Each mysqli method and each pg_* function is called on a live
// connection, with an object at each parameter that can take one and plain values elsewhere. The hooks the
// object's class runs (__toString, __serialize, __sleep, jsonSerialize) are counted.
// Run: php sql-connected.php OUT.tsv

final class SqlCounter implements JsonSerializable
{
    public static array $hits = [];

    public function __serialize(): array
    {
        self::$hits['__serialize'] = (self::$hits['__serialize'] ?? 0) + 1;
        return ['v' => 1];
    }

    public function __sleep(): array
    {
        self::$hits['__sleep'] = (self::$hits['__sleep'] ?? 0) + 1;
        return [];
    }

    public function __toString(): string
    {
        self::$hits['__tostring'] = (self::$hits['__tostring'] ?? 0) + 1;
        return 'x';
    }

    public function jsonSerialize(): mixed
    {
        self::$hits['jsonserialize'] = (self::$hits['jsonserialize'] ?? 0) + 1;
        return ['v' => 1];
    }
}

const SQL_SKIP = [
    // Reconnect or close the connection, or change server state the other calls depend on.
    'connect', 'real_connect', 'init', 'close', 'kill', 'change_user', 'debug', 'dump_debug_info', 'refresh',
    'shutdown', 'stat', 'options', 'ssl_set', 'pg_connect', 'pg_pconnect', 'pg_close', 'pg_connect_poll',
    'pg_connection_reset', 'pg_cancel_query', 'pg_socket', 'pg_socket_poll', 'pg_get_notify', 'pg_flush',
    // Read or write files of the host, or enter a COPY state that the next calls would fail in.
    'pg_trace', 'pg_untrace', 'pg_lo_import', 'pg_lo_export', 'pg_copy_from', 'pg_copy_to', 'pg_put_line',
    'pg_end_copy', 'pg_put_copy_data', 'pg_put_copy_end', 'pg_consume_input', 'pg_connection_busy',
    // Block until a result is ready.
    'poll', 'reap_async_query', 'use_result',
];

function probe_call(callable $call, array $args): array
{
    SqlCounter::$hits = [];
    pcntl_alarm(2);
    try {
        $call(...$args);
        $result = 'ok';
    } catch (Throwable $e) {
        $result = 'threw:' . substr(get_class($e) . ':' . $e->getMessage(), 0, 60);
    }
    pcntl_alarm(0);
    return [SqlCounter::$hits ? implode(',', array_keys(SqlCounter::$hits)) : '-', str_replace("\t", ' ', $result)];
}

pcntl_async_signals(true);
pcntl_signal(SIGALRM, static function (): void { throw new RuntimeException('alarm'); });

$out = fopen($argv[1], 'w');

// MySQL: the connection is the object of each mysqli method.
$m = new mysqli('127.0.0.1', 'root', '', '', 13306);
$m->query('CREATE DATABASE IF NOT EXISTS probe_experiments');
$m->select_db('probe_experiments');
foreach ((new ReflectionClass('mysqli'))->getMethods(ReflectionMethod::IS_PUBLIC) as $method) {
    $name = strtolower($method->getName());
    if (in_array($name, SQL_SKIP, true) || $method->isStatic() || str_starts_with($name, '__')) {
        continue;
    }
    foreach ($method->getParameters() as $at => $param) {
        $args = [];
        foreach ($method->getParameters() as $i => $p) {
            if ($i > $at && $p->isOptional()) {
                break;
            }
            $args[$i] = $i === $at ? new SqlCounter() : 'x';
        }
        [$hooks, $result] = probe_call(static fn (...$a) => $m->$name(...$a), $args);
        fwrite($out, "mysqli::$name\t$at\t$hooks\t$result\n");
    }
}

// PostgreSQL: the connection is the first argument of each pg_* function.
$pg = pg_connect('host=127.0.0.1 port=15432 user=torres dbname=postgres');
foreach (get_defined_functions()['internal'] as $fn) {
    if (!str_starts_with($fn, 'pg_') || in_array($fn, SQL_SKIP, true)) {
        continue;
    }
    $ref = new ReflectionFunction($fn);
    foreach ($ref->getParameters() as $at => $param) {
        if ($at === 0) {
            continue;
        }
        $args = [$pg];
        foreach ($ref->getParameters() as $i => $p) {
            if ($i === 0) {
                continue;
            }
            if ($i > $at && $p->isOptional()) {
                break;
            }
            $args[$i] = $i === $at ? new SqlCounter() : 'x';
        }
        [$hooks, $result] = probe_call(static fn (...$a) => $fn(...$a), $args);
        fwrite($out, "$fn\t$at\t$hooks\t$result\n");
    }
}
fclose($out);
echo "done\n";

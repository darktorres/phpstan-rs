<?php
// Connected Redis facts: each method, against a scratch server, under each serializer, with an object at each
// parameter. Run: php redis-connected.php OUT.tsv, with a Redis server on 127.0.0.1:16379 (see README.md).
// The hooks are counted in the object, so user code that runs is the only thing recorded.
// object, the object is passed there and the other parameters get plain values. The hooks the
// object's class runs (__serialize, __sleep, __toString, jsonSerialize) are counted per serializer.
// Blocking and server-changing methods are not called.

final class Counter implements JsonSerializable
{
    public static array $hits = [];

    public function __serialize(): array
    {
        self::$hits['__serialize'] = (self::$hits['__serialize'] ?? 0) + 1;
        return ['v' => 1];
    }

    public function __unserialize(array $data): void
    {
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

const SKIP = [
    '__construct', '__destruct', '__call', '__callstatic', 'close', 'connect', 'pconnect', 'popen', 'open',
    'subscribe', 'psubscribe', 'ssubscribe', 'monitor', 'blpop', 'brpop', 'brpoplpush', 'blmove', 'bzpopmin',
    'bzpopmax', 'bzmpop', 'blmpop', 'wait', 'waitaof', 'flushall', 'flushdb', 'shutdown', 'slaveof', 'replicaof',
    'failover', 'bgrewriteaof', 'bgsave', 'save', 'config', 'debug', 'migrate', 'sync', 'psync', 'client', 'script',
    'function', 'eval', 'eval_ro', 'evalsha', 'evalsha_ro', 'fcall', 'fcall_ro', 'auth', 'select', 'swapdb', 'acl',
    'module', 'cluster', 'readonly', 'readwrite', 'reset', 'multi', 'exec', 'discard', 'pipeline', 'watch', 'unwatch',
    'unsubscribe', 'punsubscribe', 'sunsubscribe', 'setoption', 'getoption', 'rawcommand', 'command', 'bitop',
];

$serializers = ['none' => Redis::SERIALIZER_NONE, 'php' => Redis::SERIALIZER_PHP,
    'igbinary' => Redis::SERIALIZER_IGBINARY, 'json' => Redis::SERIALIZER_JSON];

$r = new Redis();
$r->connect('127.0.0.1', 16379);
$out = fopen($argv[1], 'w');
$methods = array_filter((new ReflectionClass('Redis'))->getMethods(ReflectionMethod::IS_PUBLIC),
    static fn (ReflectionMethod $m): bool => !$m->isStatic() && !in_array(strtolower($m->getName()), SKIP, true));
pcntl_async_signals(true);
pcntl_signal(SIGALRM, static function (): void { throw new RuntimeException('alarm'); });

foreach ($methods as $method) {
    $name = strtolower($method->getName());
    $params = $method->getParameters();
    foreach ($params as $at => $param) {
        foreach ($serializers as $sname => $sconst) {
            $r->flushdb();
            $r->setOption(Redis::OPT_SERIALIZER, $sconst);
            Counter::$hits = [];
            $args = [];
            foreach ($params as $i => $p) {
                if ($i > $at && $p->isOptional()) {
                    break;
                }
                $args[$i] = $i === $at ? new Counter() : (str_contains(strtolower($p->getName()), 'key') ? 'k' : 'x');
                if ($i !== $at && $p->getType() !== null && in_array(strtolower((string) $p->getType()), ['int', '?int'], true)) {
                    $args[$i] = 1;
                }
                if ($i !== $at && $p->getType() !== null && in_array(strtolower((string) $p->getType()), ['array', '?array'], true)) {
                    $args[$i] = ['x'];
                }
            }
            pcntl_alarm(2);
            try {
                $r->$name(...$args);
                $result = 'ok';
            } catch (Throwable $e) {
                $result = 'threw:' . substr(get_class($e) . ':' . $e->getMessage(), 0, 60);
            }
            pcntl_alarm(0);
            $hooks = Counter::$hits ? implode(',', array_keys(Counter::$hits)) : '-';
            fwrite($out, implode("\t", [$name, $at, $sname, $hooks, str_replace("\t", ' ', $result)]) . "\n");
        }
    }
}
fclose($out);
echo "methods: ", count($methods), "\n";

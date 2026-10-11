<?php
// The values a parameter is probed with, and the lines a probe's call yields.

require_once __DIR__ . '/stubdocs.php';

/**
 * Whether the stubs document an untyped parameter as a resource. PHP's
 * reflection shows no type for it, so the stubs' docblocks say what it takes.
 */
function documented_resource(ReflectionParameter $param): bool
{
    static $map = null;
    $map ??= untyped_resource_params(phpstorm_stubs_dir());
    $function = $param->getDeclaringFunction();
    $key = $function instanceof ReflectionMethod
        ? strtolower($function->getDeclaringClass()->getName() . '::' . $function->getName())
        : strtolower($function->getName());
    return isset($map[$key][$param->getName()]);
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

/**
 * Whether `$type` admits a value of the class `$class`: `mixed` and no type do,
 * `object` does, a type the class is an instance of does (an interface it
 * implements, or its own class), and `iterable` does for a `Traversable`.
 */
function accepts_value(?ReflectionType $type, string $class): bool
{
    if ($type === null) {
        return true;
    }
    if ($type instanceof ReflectionNamedType) {
        $t = strtolower($type->getName());
        return $t === 'mixed' || $t === 'object'
            || ($t === 'iterable' && is_a($class, Traversable::class, true))
            || is_a($class, $type->getName(), true);
    }
    foreach ($type->getTypes() as $member) {
        if (accepts_value($member, $class)) {
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
    if (accepts_value($type, RecordingObject::class)) {
        $probes[] = ['object', static fn () => new RecordingObject()];
    }
    if (accepts_value($type, RecordingIterator::class)) {
        $probes[] = ['iterator', static fn () => new RecordingIterator()];
    }
    if (accepts_value($type, RecordingSleeper::class)) {
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

/**
 * A valid value of a class or resource type that no plain value stands for,
 * made fresh by the child that calls with it: a stream, an XML writer, an
 * Intl formatter. Keyed by the lowercased name of the type. Only the types a
 * plain value cannot satisfy are here; each value needs no server, file or
 * network. A union has no entry: its scalar members are plain values.
 */
function valid_value(?ReflectionType $type): ?Closure
{
    static $values = null;
    if (!$type instanceof ReflectionNamedType) {
        return null;
    }
    $values ??= [
        'xmlwriter' => static fn (): XMLWriter => xmlwriter_open_memory(),
        'xmlparser' => static fn (): XMLParser => xml_parser_create(),
        // Made in the worker's own IPC namespace, which the sandbox gives it; without one the queue is the host's.
        'sysvmessagequeue' => static fn (): SysvMessageQueue => msg_get_queue(0x5150),
        'datetimeinterface' => static fn (): DateTimeImmutable => new DateTimeImmutable('2000-01-01 00:00:00', new DateTimeZone('UTC')),
        'datetime' => static fn (): DateTime => new DateTime('2000-01-01 00:00:00', new DateTimeZone('UTC')),
        'dateinterval' => static fn (): DateInterval => new DateInterval('P1D'),
        'gdimage' => static fn (): mixed => imagecreatetruecolor(1, 1),
        'finfo' => static fn (): finfo => new finfo(FILEINFO_MIME_TYPE),
        'hashcontext' => static fn (): HashContext => hash_init('md5'),
        'curlhandle' => static fn (): CurlHandle => curl_init(),
        'curlmultihandle' => static fn (): CurlMultiHandle => curl_multi_init(),
        'numberformatter' => static fn (): NumberFormatter => numfmt_create('en', NumberFormatter::DECIMAL),
        'intldateformatter' => static fn (): IntlDateFormatter => datefmt_create('en', IntlDateFormatter::FULL, IntlDateFormatter::FULL, 'UTC'),
        'collator' => static fn (): Collator => collator_create('en'),
        'intltimezone' => static fn (): IntlTimeZone => intltz_create_default(),
        'intlcalendar' => static fn (): IntlCalendar => intlcal_create_instance('UTC', 'en'),
        'messageformatter' => static fn (): MessageFormatter => msgfmt_create('en', 'x'),
        'deflatecontext' => static fn (): DeflateContext => deflate_init(ZLIB_ENCODING_RAW),
        'inflatecontext' => static fn (): InflateContext => inflate_init(ZLIB_ENCODING_RAW),
        'imagick' => static fn (): Imagick => new Imagick(),
        'imagickdraw' => static fn (): ImagickDraw => new ImagickDraw(),
        'imagickpixel' => static fn (): ImagickPixel => new ImagickPixel('white'),
    ];
    return $values[strtolower($type->getName())] ?? null;
}

/** A plain value for a parameter a probe is not at. */
function plain_value(ReflectionParameter $param): mixed
{
    $type = $param->getType();
    if ($type === null) {
        // A memory stream serves every resource kind the stubs name: PHP takes
        // a stream where it takes a stream context, and a function that needs
        // another kind throws, which the table records.
        return documented_resource($param) ? fopen('php://memory', 'r+') : 1;
    }
    if ($type->allowsNull() && !admits($type, 'int') && !admits($type, 'string')) {
        return null;
    }
    $valid = valid_value($type);
    if ($valid !== null) {
        return $valid();
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

/** Where a probe is: its parameter's position, and `[]` for the elements of an array. */
function where(string $prefix, int $at, string $probe): string
{
    return $prefix . $at . (in_array($probe, ['objects', 'callables', 'urls'], true) ? '[]' : '');
}

/** The lines for one probe's call: `probed` once the call was reached. */
function record(string $builtin, string $where, ?array $seen, array &$out): void
{
    Stats::child($builtin, $where);
    if ($seen === null) {
        Stats::failed($builtin, $where);
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

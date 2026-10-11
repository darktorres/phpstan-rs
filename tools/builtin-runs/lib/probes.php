<?php
// The values a parameter is probed with, and the lines a probe's call yields.

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

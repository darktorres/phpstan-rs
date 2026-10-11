<?php
// The lines for one unit: a function, a method, or a method after another.

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
 * The object a held unit's children call their methods on: made by `$maker`,
 * then given the probe `$make` makes at `$at` by calling `$firstName`. The
 * object is `null` when `$maker` gives none. The probe comes back with the
 * object, so that it lives as long as the object does in the child.
 *
 * @param list<ReflectionParameter> $firstParams
 * @return array{?object, mixed}
 */
function held_first(callable $maker, string $firstName, array $firstParams, int $at, callable $make): array
{
    $object = $maker();
    if (!is_object($object)) {
        return [null, null];
    }
    $value = $make();
    try {
        $object->$firstName(...arguments($firstParams, $at, $value));
    } catch (Throwable) {
    }
    return [$object, $value];
}

/**
 * The lines for the methods of a class called after `$firstName` was given
 * the probe `$probeAt` of its parameter `$paramAt`: what each runs of what
 * the first kept. With `$skip` `dead`, a survey child first makes the object
 * and calls the first method: when that yields no object, the methods are
 * not run (each would stop at the same point and record nothing).
 */
function probe_held(string $class, string $firstName, int $paramAt, int $probeAt, callable $wanted, string $skip, array &$out): void
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
            $targets = array_values(array_filter(
                $instance,
                static fn (ReflectionMethod $then): bool => $wanted(strtolower($class . '::' . $then->getName())),
            ));
            if ($targets === []) {
                return;
            }
            $where = 'by:' . strtolower($firstName) . ':' . where('', $at, $probe);
            $maker = $makers[0][1];
            if ($skip === 'dead') {
                $survey = in_child(static function () use ($maker, $firstName, $firstParams, $at, $make): void {
                    [$object, $value] = held_first($maker, $firstName, $firstParams, $at, $make);
                    if ($object !== null) {
                        Seen::$entries["\tsurvey\t"] = true;
                    }
                });
                Stats::child(strtolower($class . '::' . $firstName), 'survey:' . $where);
                if ($survey === null || !isset($survey["\tsurvey\t"])) {
                    Stats::skipped('dead', count($targets));
                    return;
                }
            }
            foreach ($targets as $then) {
                $builtin = strtolower($class . '::' . $then->getName());
                $thenName = $then->getName();
                $thenParams = $then->getParameters();
                $seen = in_child(static function () use ($maker, $firstName, $firstParams, $at, $make, $where, $thenName, $thenParams): void {
                    [$object, $value] = held_first($maker, $firstName, $firstParams, $at, $make);
                    if ($object === null) {
                        return;
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

/** Runs one unit, giving back the lines it found. */
function run_unit(array $unit, callable $wanted, string $skip): array
{
    $out = [];
    [$kind, $name, $method, $paramAt, $probeAt] = $unit;
    Stats::$unit = $kind;
    match ($kind) {
        'function' => probe_function($name, $out),
        'method' => probe_method($name, $method, $out),
        'held' => probe_held($name, $method, $paramAt, $probeAt, $wanted, $skip, $out),
    };
    return $out;
}

<?php
// The lines for one unit: a function, a method, or a method after another.

/**
 * One child of a unit: run it, and record what it gave, unless a retry asks
 * for other children.
 *
 * @param array<string, true> $out
 */
function probe_child(string $builtin, string $where, Closure $call, array &$out, ?string $probe = null): void
{
    if (Stats::wants($builtin, $where)) {
        Stats::$probe = $probe;
        record($builtin, $where, in_child($call), $out);
    }
}

/** The lines for a function. */
function probe_function(string $function, array &$out): void
{
    $builtin = strtolower($function);
    $reflection = new ReflectionFunction($function);
    $params = $reflection->getParameters();
    foreach (call_modes($params, -1) as $valued) {
        probe_child($builtin, 'plain', static function () use ($function, $params, $valued): void {
            Seen::$where = 'plain';
            Seen::add('reached');
            invoke_arguments($params, -1, null, static fn (array $args): mixed => $function(...$args), $valued);
            Seen::returned();
        }, $out);
    }
    foreach ($params as $at => $param) {
        foreach (probes_for($param) as [$probe, $make]) {
            $where = where('', $at, $probe);
            foreach (call_modes($params, $at) as $valued) {
                probe_child($builtin, $where, static function () use ($function, $params, $at, $make, $where, $probe, $valued): void {
                    $weak = null;
                    // What the call gave back and the other arguments, which a
                    // caller still holds once it returned.
                    $kept = (static function () use ($function, $params, $at, $make, $where, $valued, &$weak): array {
                        $value = $make();
                        if (is_object($value)) {
                            $weak = WeakReference::create($value);
                        }
                        Seen::$entries = [];
                        Seen::$where = $where;
                        $refs = [];
                        Seen::add('reached');
                        $result = invoke_arguments($params, $at, $value, static function (array $args) use ($function, &$refs): mixed {
                            $refs = [];
                            foreach ($args as $i => $arg) {
                                $refs[$i] = $arg;
                                $args[$i] = &$refs[$i];
                            }
                            return call_user_func_array($function, $args);
                        }, $valued);
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
                }, $out, $probe);
            }
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
    foreach (call_modes($params, -1) as $valued) {
        probe_child($builtin, 'plain', static function () use ($makers, $static, $class, $name, $params, $valued): void {
            $object = $static ? null : method_receiver(new ReflectionClass($class), $name, $makers[0][1]);
            if (!$static && $object === null) {
                return;
            }
            Seen::$entries = [];
            Seen::$where = 'plain';
            Seen::add('reached');
            invoke_arguments($params, -1, null, static fn (array $args): mixed => $static ? $class::$name(...$args) : $object->$name(...$args), $valued);
            Seen::returned();
        }, $out);
    }
    // A probe at each of the method's parameters, on a plain object.
    foreach ($params as $at => $param) {
        foreach (probes_for($param) as [$probe, $make]) {
            $where = where('', $at, $probe);
            foreach (call_modes($params, $at) as $valued) {
                probe_child($builtin, $where, static function () use ($makers, $static, $class, $name, $params, $at, $make, $where, $valued): void {
                    $object = $static ? null : method_receiver(new ReflectionClass($class), $name, $makers[0][1]);
                    if (!$static && $object === null) {
                        return;
                    }
                    $value = $make();
                    Seen::$entries = [];
                    Seen::$where = $where;
                    Seen::add('reached');
                    invoke_arguments($params, $at, $value, static fn (array $args): mixed => $static ? $class::$name(...$args) : $object->$name(...$args), $valued);
                    Seen::returned();
                }, $out, $probe);
            }
        }
    }
    if ($static) {
        return;
    }
    // The method, plainly, on an object made with a probe.
    foreach (array_slice($makers, 1) as [$where, $make]) {
        foreach (call_modes($params, -1) as $valued) {
            probe_child($builtin, $where, static function () use ($make, $where, $name, $params, $valued): void {
                Seen::$where = $where;
                $object = $make();
                Seen::$entries = [];
                Seen::add('reached');
                invoke_arguments($params, -1, null, static fn (array $args): mixed => $object->$name(...$args), $valued);
                Seen::returned();
            }, $out);
        }
    }
}

/**
 * The object a held unit's children call their methods on: made by `$maker`,
 * then given the probe `$make` makes at `$at` by calling `$firstName`. The
 * object is `null` when `$maker` gives none. The probe comes back with the
 * object, so that it lives as long as the object does in the child.
 *
 * The weak references to the probe's objects are taken before the call, for
 * `released_probe`.
 *
 * @param list<ReflectionParameter> $firstParams
 * @return array{?object, mixed, list<WeakReference<object>>}
 */
function held_first(callable $maker, string $firstName, array $firstParams, int $at, callable $make, bool $valued = false): array
{
    $object = $maker();
    if (!is_object($object)) {
        return [null, null, []];
    }
    $value = $make();
    $witness = array_map(
        static fn (object $element): WeakReference => WeakReference::create($element),
        array_values(array_filter(is_array($value) ? $value : [$value], 'is_object')),
    );
    try {
        invoke_arguments($firstParams, $at, $value, static fn (array $args): mixed => $object->$firstName(...$args), $valued);
    } catch (Throwable) {
    }
    return [$object, $value, $witness];
}

/**
 * Whether no reference to a probe is left, once the child dropped its own:
 * the probe's objects are gone, or its stream is closed. A `url` or `urls`
 * probe is never released, since a string copied by a builtin cannot be seen.
 *
 * @param list<WeakReference<object>> $witness
 */
function released_probe(string $probe, array $witness): bool
{
    if ($probe === 'stream') {
        return Seen::$closed > 0;
    }
    if ($witness === [] || !in_array($probe, ['callable', 'object', 'iterator', 'sleeper', 'objects', 'callables'], true)) {
        return false;
    }
    foreach ($witness as $weak) {
        if ($weak->get() !== null) {
            return false;
        }
    }
    return true;
}

/**
 * The lines for the methods of a class called after `$firstName` was given
 * the probe `$probeAt` of its parameter `$paramAt`: what each runs of what
 * the first kept. With `$skip` `dead` or `released`, a survey child first makes
 * the object and calls the first method: when that yields no object, the
 * methods are not run (each would stop at the same point and record nothing).
 * With `released`, when the survey finds the probe released, a method whose
 * builtin already has a `probed` row in `$known` is not run: the probe can
 * reach nothing it holds.
 *
 * @param array<string, int> $known the builtins of the class that have a `probed` row
 */
function probe_held(string $class, string $firstName, int $paramAt, int $probeAt, callable $wanted, string $skip, array $known, array &$out): void
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
                static fn (ReflectionMethod $then): bool => $wanted(strtolower($class . '::' . $then->getName())) && !in_array(strtolower($class . '::' . $then->getName()), METHODS_SKIPPED, true),
            ));
            if ($targets === []) {
                return;
            }
            $where = 'by:' . strtolower($firstName) . ':' . where('', $at, $probe);
            $maker = $makers[0][1];
            $surveyBuiltin = strtolower($class . '::' . $firstName);
            $surveyWhere = 'survey:' . $where;
            // Each mode of the first call is a unit of its own: a dead first call in one mode does not end the others.
            foreach (call_modes($firstParams, $at) as $valued) {
                $survey = null;
                if ($skip !== 'off') {
                    $survey = in_child(static function () use ($maker, $firstName, $firstParams, $at, $make, $probe, $valued): void {
                        [$object, $value, $witness] = held_first($maker, $firstName, $firstParams, $at, $make, $valued);
                        if (!is_object($object)) {
                            return;
                        }
                        Seen::$entries["\tsurvey\t"] = true;
                        unset($value);
                        gc_collect_cycles();
                        if (released_probe($probe, $witness)) {
                            Seen::$entries["\treleased\t"] = true;
                        }
                    });
                    Stats::child($surveyBuiltin, $surveyWhere);
                    if ($survey === null) {
                        Stats::failed($surveyBuiltin, $surveyWhere);
                    }
                    if ($survey === null || !isset($survey["\tsurvey\t"])) {
                        Stats::skipped('dead', count($targets));
                        continue;
                    }
                }
                // A retry that found the survey failed runs every method of the unit.
                $surveyRetried = Stats::wants($surveyBuiltin, $surveyWhere);
                $released = $skip === 'released' && isset($survey["\treleased\t"]);
                foreach ($targets as $then) {
                    $builtin = strtolower($class . '::' . $then->getName());
                    if ($released && isset($known[$builtin])) {
                        Stats::skipped('released', 1);
                        continue;
                    }
                    if (!$surveyRetried && !Stats::wants($builtin, $where)) {
                        continue;
                    }
                    $thenName = $then->getName();
                    $thenParams = $then->getParameters();
                    foreach (call_modes($thenParams, -1) as $thenValued) {
                        probe_child($builtin, $where, static function () use ($maker, $firstName, $firstParams, $at, $make, $where, $thenName, $thenParams, $valued, $thenValued): void {
                            [$object, $value] = held_first($maker, $firstName, $firstParams, $at, $make, $valued);
                            if ($object === null) {
                                return;
                            }
                            Seen::$entries = [];
                            Seen::$where = $where;
                            Seen::add('reached');
                            invoke_arguments($thenParams, -1, null, static fn (array $args): mixed => $object->$thenName(...$args), $thenValued);
                            Seen::returned();
                        }, $out, $probe);
                    }
                }
            }
        }
    }
}

/**
 * Runs one unit, giving back the lines it found.
 *
 * @param array<string, int> $known the builtins of the unit's class that have a `probed` row
 */
function run_unit(array $unit, callable $wanted, string $skip, array $known): array
{
    $out = [];
    [$kind, $name, $method, $paramAt, $probeAt] = $unit;
    Stats::$unit = $kind;
    match ($kind) {
        'function' => probe_function($name, $out),
        'method' => probe_method($name, $method, $out),
        'held' => probe_held($name, $method, $paramAt, $probeAt, $wanted, $skip, $known, $out),
    };
    return $out;
}

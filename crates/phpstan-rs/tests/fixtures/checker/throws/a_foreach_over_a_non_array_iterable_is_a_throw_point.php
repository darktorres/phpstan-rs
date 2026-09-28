<?php
interface Coercer { public function coerce(mixed $v): mixed; }
/** @param iterable<mixed> $value */
function f(iterable $value, Coercer $c): string {
    $iterating = true;
    try {
        foreach ($value as $v) {
            $iterating = false;
            $c->coerce($v);
            $iterating = true;
        }
    } catch (Throwable $e) {
        if ($iterating) {
            return 'iteration';
        }
        return match (true) {
            $iterating => 'iteration',
            default => 'value',
        };
    }
    return 'ok';
}
/** @param list<int> $value */
function g(array $value, Coercer $c): string {
    $iterating = true;
    try {
        foreach ($value as $v) {
            $iterating = false;
            $c->coerce($v);
            $iterating = true;
        }
    } catch (Throwable $e) {
        if ($iterating) {
            return 'never';
        }
    }
    return 'ok';
}

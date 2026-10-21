<?php
/** @param array<mixed> $values */
function walk(array $values): void {
    $walk = null;
    $walk = function (mixed $value) use (&$walk): void {
        if (is_array($value)) {
            foreach ($value as $v) {
                $walk($v);
            }
        }
    };
    $walk($values);
    $walk = null;
}

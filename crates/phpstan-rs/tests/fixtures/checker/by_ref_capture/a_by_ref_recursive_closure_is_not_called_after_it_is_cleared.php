<?php
function walk(array<mixed> $values): void {
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

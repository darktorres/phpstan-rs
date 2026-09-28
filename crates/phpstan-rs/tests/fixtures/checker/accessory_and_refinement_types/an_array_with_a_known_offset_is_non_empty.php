<?php
/** @param non-empty-array<mixed> $x */
function needsNonEmpty(array $x): void {}
/** @param array<mixed> $array */
function f(array $array): void {
    if (isset($array['def'])) {
        needsNonEmpty($array);
    }
    if (array_key_exists('k', $array)) {
        needsNonEmpty($array);
    }
}

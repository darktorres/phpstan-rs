<?php
function f(array $a, bool $b, \Generator $g): void {
    in_array(1, $a);
    in_array(1, $a, true);
    in_array(1, $a, $b);
    array_search(1, $a, strict: true);
    array_keys($a);
    array_keys($a, 1);
    base64_decode('x');
    base64_decode('x', true);
    iterator_to_array($g);
    iterator_to_array($g, false);
    iterator_to_array($g, preserve_keys: true);
    in_array(...[1, $a]);
}

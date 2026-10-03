<?php
/** @throws void */
function f(?int $x): int {
    return $x ?? throw new \Exception('x');
}

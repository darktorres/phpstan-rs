<?php
/** @throws void */
function f(): int {
    return 1;
    throw new \Exception('x');
}

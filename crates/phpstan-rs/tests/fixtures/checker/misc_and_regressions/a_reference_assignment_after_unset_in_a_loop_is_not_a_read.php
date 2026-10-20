<?php
function f(array $queue): void {
    $stub = null;
    for ($i = 0; $i < 3; $i++) {
        $vals = $queue;
        $vals[0] = &$stub;
        unset($stub);
    }
}

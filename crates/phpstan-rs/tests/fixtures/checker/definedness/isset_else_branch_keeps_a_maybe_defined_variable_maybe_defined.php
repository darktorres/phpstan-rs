<?php
function f(bool $c): void {
    if ($c) { $v = 1; }
    if (isset($v)) {} else { echo $v; }
}

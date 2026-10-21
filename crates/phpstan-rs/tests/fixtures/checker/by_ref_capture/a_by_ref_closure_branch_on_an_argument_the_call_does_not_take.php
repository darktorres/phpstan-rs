<?php
$x = null;
$f = function (bool $flag) use (&$x): void {
    if ($flag) {
        strlen($x);
    }
};
$f(false);

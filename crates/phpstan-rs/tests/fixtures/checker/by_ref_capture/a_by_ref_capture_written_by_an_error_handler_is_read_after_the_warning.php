<?php
$x = null;
set_error_handler(function (int $n, string $s) use (&$x): bool { $x = 'str'; return true; });
$f = function () use (&$x): void { strlen($x); };
$arr = [];
$v = $arr['k'];
$f();

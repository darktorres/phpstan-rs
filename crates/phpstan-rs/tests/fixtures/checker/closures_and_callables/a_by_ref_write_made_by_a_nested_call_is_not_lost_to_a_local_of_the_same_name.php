<?php
$x = 'top';
$w = function () use (&$x): void { $x = 'str'; };
$r = function () use (&$w): void { $x = 5; $w(); };
$r();
strlen($x);

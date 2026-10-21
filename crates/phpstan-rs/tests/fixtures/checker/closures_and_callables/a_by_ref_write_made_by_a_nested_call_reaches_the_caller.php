<?php
$x = null;
$w = function () use (&$x): void { $x = 'str'; };
$r = function () use (&$w): void { $w(); };
$r();
strlen($x);

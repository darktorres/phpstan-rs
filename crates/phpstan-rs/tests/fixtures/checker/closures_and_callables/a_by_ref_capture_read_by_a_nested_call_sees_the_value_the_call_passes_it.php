<?php
$x = null;
$w = function () use (&$x): void { strlen($x); };
$r = function () use (&$w): void { $w(); };
$r();

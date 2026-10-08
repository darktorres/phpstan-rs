<?php

use function PHPStan\Testing\assertType;

$mode = 'fast';
$callback = $_GET['callback'];
$callback();
assertType('\'fast\'', $mode);

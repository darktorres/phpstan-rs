<?php

use function PHPStan\Testing\assertType;

$mode = 'slow';
$mode = 'fast';
switchMode();
assertType('\'fast\'|42', $mode);

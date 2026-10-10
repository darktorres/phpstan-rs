<?php
use function PHPStan\Testing\assertType;

$mode = 'plain';
$other = 'plain';

function configure(): void {
    global $mode;
    require __DIR__ . '/settings.php';
}

configure();
assertType('mixed', $mode);
assertType("'plain'", $other);

<?php
namespace CallEffects\CaptureReturn;

use function PHPStan\Testing\assertType;

/** @param list<int> $lines */
function collapse(array $lines): string
{
    $startsRun = true;
    $each = function (string $out, int $line) use (&$startsRun): string {
        assertType('bool', $startsRun);
        if ($startsRun) {
            $startsRun = false;
            return $out . $line;
        }
        return $out . '..';
    };
    $out = '';
    foreach ($lines as $line) {
        $out = $each($out, $line);
    }
    return $out;
}

echo collapse([1, 2, 3]);

<?php
function takesHandle(resource $handle): void
{
}

/** @param resource $handle */
function takesDocHandle($handle): void
{
    \PHPStan\Testing\assertType('resource', $handle);
}

function main(): void
{
    $h = fopen('php://memory', 'r');
    if ($h !== false) {
        takesHandle($h);
        takesDocHandle($h);
    }
}

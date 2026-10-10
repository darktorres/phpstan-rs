<?php
use function PHPStan\Testing\assertType;

$written = ['x' => 1];
$removed = ['x' => 1];
$passed = ['x' => 1];

function write(string $key): void {
    global $written;
    $written[$key] = 'str';
}

function remove(string $key): void {
    global $removed;
    unset($removed[$key]);
}

function fill(string $key): void {
    global $passed;
    preg_match('/a/', 'a', $passed[$key]);
}

function readAll(): void {
    global $written, $removed, $passed;
    assertType('mixed', $written['x']);
    assertType('mixed', $removed['x']);
    assertType('mixed', $passed['x']);
}

write('x');
remove('x');
fill('x');
readAll();

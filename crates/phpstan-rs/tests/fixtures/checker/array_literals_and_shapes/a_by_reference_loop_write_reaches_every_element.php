<?php
use function PHPStan\Testing\assertType;

/**
 * @param array<int, array{id: int}> $infos
 */
function each(array $infos): void {
    foreach ($infos as &$info) {
        $info['src'] = 'x';
    }
    unset($info);
    assertType("array<int, array{id: int, src: 'x'}>", $infos);
}

/**
 * @param array<int, array{id: int}> $infos
 */
function until(array $infos): void {
    foreach ($infos as &$info) {
        if ($info['id'] > 3) {
            break;
        }
        $info['src'] = 'x';
    }
    unset($info);
    assertType("array<int, array{id: int, src?: 'x'}>", $infos);
}

each([['id' => 1], ['id' => 2]]);
until([['id' => 1], ['id' => 5]]);

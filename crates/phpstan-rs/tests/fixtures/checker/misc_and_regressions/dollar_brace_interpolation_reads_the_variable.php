<?php
use function PHPStan\Testing\assertType;
function f(int $name, string $s): void {
    assertType('string', "${name}");
    assertType('string', "a ${s} b");
    $arr = ['k' => 1];
    assertType('string', "${arr['k']}");
    $v = 'name';
    echo "${$v}";
    echo "${nope}";
    assertType("'\${literal}'", "\${literal}");
}

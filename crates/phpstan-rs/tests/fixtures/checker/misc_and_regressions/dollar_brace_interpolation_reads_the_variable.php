<?php
use function PHPStan\Testing\assertType;
function f(int $name, string $s): void {
    assertType('decimal-int-string', "${name}");
    assertType('non-falsy-string', "a ${s} b");
    $arr = ['k' => 1];
    assertType("'1'", "${arr['k']}");
    $v = 'name';
    echo "${$v}";
    echo "${nope}";
    assertType("'\${literal}'", "\${literal}");
}

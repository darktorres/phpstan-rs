<?php
use function PHPStan\Testing\assertType;
const BIN = "\xff";
function f(): void {
    $a = ["\xff" => 1, "\xfe" => 2];
    assertType('array{"\\xff": 1, "\\xfe": 2}', $a);
    assertType('1', $a["\xff"]);
    assertType('"\\xff"', BIN);
    assertType("'it\\'s'", "it's");
    assertType('"a\\nb"', "a\nb");
    $d = ["\xff" => 1, "\xff" => 2];
}

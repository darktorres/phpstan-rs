<?php
use function PHPStan\Testing\assertType;
function f(string $u): void {
    if ($u !== '') {
        $_SESSION['k'] = $u;
    }
    echo $_SESSION;
    assertType('mixed', $_SESSION['other']);
}

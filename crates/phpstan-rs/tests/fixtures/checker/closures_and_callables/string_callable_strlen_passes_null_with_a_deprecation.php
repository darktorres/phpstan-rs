<?php
function f(): void {
    $fn = 'strlen';
    $fn(null);
    assertType('3', $fn('abc'));
}
f();

<?php
function f(): void {
    $a = '\strlen';
    assertType('3', $a('abc'));
    $b = 'STRLEN';
    assertType('3', $b('abc'));
}

<?php
function f(): void {
    $fn = 'strlen';
    assertType('int<0, max>', $fn(12345));
}

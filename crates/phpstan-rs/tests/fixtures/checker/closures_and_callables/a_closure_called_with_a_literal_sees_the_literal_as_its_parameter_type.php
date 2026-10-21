<?php
$g = function (?string $s): void {
    assertType("'a'", $s);
};
$g('a');

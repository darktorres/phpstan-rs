<?php
$x = null;
$c = function (): void {
    global $x;
    strlen($x);
};
$c();

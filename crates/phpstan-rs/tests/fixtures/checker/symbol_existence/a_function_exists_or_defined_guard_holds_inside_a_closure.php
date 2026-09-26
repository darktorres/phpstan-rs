<?php
function f(): void
{
    if (function_exists('my_missing_fn')) {
        $g = function () {
            return my_missing_fn();
        };
    }
    if (defined('MY_MISSING_CONST')) {
        $h = function () {
            return MY_MISSING_CONST;
        };
    }
    $k = function () {
        return my_missing_fn();
    };
}

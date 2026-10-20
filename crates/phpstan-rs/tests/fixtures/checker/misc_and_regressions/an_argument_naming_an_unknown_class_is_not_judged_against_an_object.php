<?php
/**
 * @param Missing $m
 */
function take($m): void {
}
function g(object $o): void {
    take($o);
    take(new \stdClass());
}

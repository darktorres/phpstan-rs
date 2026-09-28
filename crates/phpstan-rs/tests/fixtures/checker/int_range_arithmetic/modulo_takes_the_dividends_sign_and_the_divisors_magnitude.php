<?php
use function PHPStan\Testing\assertType;
/**
 * @param int<0, max> $nn
 * @param int<-10, -1> $neg
 * @param int<0, 2> $small
 * @param int<-3, 5> $mixed
 * @param int<-3, 7> $div
 */
function f(int $nn, int $neg, int $small, int $mixed, int $div, int $any): void {
    assertType('int<0, 2>', $nn % -3);
    assertType('int<0, 2>', $nn % 3);
    assertType('int<-3, 0>', $neg % 4);
    assertType('int<-10, 0>', $neg % $any);
    assertType('int<0, 2>', $small % 5);
    assertType('int<-3, 5>', $mixed % $div);
    assertType('0', $any % 1);
    assertType('0', $any % -1);
    assertType('int<-6, 6>', $any % $div);
    assertType('int', $any % $any);
    assertType('int<0, max>', $nn % $any);
}

<?php
/** @param int<0, 0> $value */
function needsZero(int $value): void {}

function testEmptyStringLength(): void {
    needsZero(strlen(''));
}

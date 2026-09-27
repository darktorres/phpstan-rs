<?php
function f(bool $b, ?int $n): void {
    printf("%s %d\n", $b, $n);
    printf("%s\n", null);
}

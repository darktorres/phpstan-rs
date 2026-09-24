<?php
function f(): void {
    a(); // @phpstan-ignore function.notFound, argument.type
    $x = 1; // @phpstan-ignore-line
    // @phpstan-ignore-next-line
    b();
}

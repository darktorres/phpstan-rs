<?php
function f(): void {
    a(); // @phpstan-ignore function.notFound
    // @phpstan-ignore-next-line
    b();
}

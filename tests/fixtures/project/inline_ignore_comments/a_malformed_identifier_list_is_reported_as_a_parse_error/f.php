<?php
function f(): void {
    totallyUndefinedFunction(); // @phpstan-ignore function.notFound,
}

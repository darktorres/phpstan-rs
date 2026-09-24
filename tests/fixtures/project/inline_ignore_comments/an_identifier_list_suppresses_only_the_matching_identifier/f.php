<?php
function f() {
    totallyUndefinedFunction(); // @phpstan-ignore function.notFound
}

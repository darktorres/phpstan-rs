<?php
function f() {
    totallyUndefinedFunction(); // @phpstan-ignore-line
    anotherUndefinedFunction();
}

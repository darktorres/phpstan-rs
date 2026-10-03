<?php
function f(): int {
    do {
        return 1;
    } while (random_int(0, 1) === 1);
}

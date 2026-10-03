<?php
function f(): int {
    try {
        echo 'work';
    } finally {
        return 1;
    }
}

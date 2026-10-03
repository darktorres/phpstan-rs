<?php
function f(array $rows): int {
    try {
        foreach ($rows as $row) {
            if ($row) {
                break;
            }
        }
    } finally {
        return 2;
    }
}

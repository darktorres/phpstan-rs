<?php
function f(array $rows): int {
    try {
        foreach ($rows as $row) {
            if ($row) {
                return 1;
            }
        }
    } finally {
        return 2;
    }
}

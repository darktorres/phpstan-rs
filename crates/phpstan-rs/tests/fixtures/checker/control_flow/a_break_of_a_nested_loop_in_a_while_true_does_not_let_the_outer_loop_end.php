<?php
function f(array $rows): void {
    while (true) {
        foreach ($rows as $row) {
            if ($row) {
                break;
            }
        }
    }
}

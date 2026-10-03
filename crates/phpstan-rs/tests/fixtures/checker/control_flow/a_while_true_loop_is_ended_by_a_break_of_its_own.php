<?php
function f(array $items): int {
    while (true) {
        foreach ($items as $item) {
            if ($item) {
                break 2;
            }
        }
    }
}

<?php
/** @param array<int, int> $data */
function heapify(int $node, array $data): void {
    while ($node !== 0 && $data[$parent = ($node - 1) >> 1] > 0) {
        echo $parent;
        $node = $parent;
    }
}

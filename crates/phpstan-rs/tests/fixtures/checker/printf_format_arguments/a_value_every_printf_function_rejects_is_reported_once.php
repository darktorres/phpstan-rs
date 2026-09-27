<?php
/**
 * @param array<string, mixed> $row
 * @param resource $stream
 */
function f(mixed $m, array $row, $stream): void {
    printf("%d\n", $m);
    printf("%s\n", $row['x']);
    echo sprintf("%d", $m);
    fprintf($stream, "%s", $m);
}

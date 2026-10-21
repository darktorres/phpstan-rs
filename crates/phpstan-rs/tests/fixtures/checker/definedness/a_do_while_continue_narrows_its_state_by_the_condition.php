<?php
function read(): ?string {
    return rand(0, 1) === 1 ? 'chunk' : null;
}
function f(): void {
    $chunk = read();
    if ($chunk === null) {
        return;
    }
    do {
        $buffer = $chunk;
        $chunk = read();
        if (strlen($buffer) === 0) {
            continue;
        }
    } while ($chunk !== null);
}

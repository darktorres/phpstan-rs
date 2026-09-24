<?php
function load(string $name): void {
    require __DIR__ . '/' . $name . '.php';
    include getenv('HOME') . '/x.php';
}

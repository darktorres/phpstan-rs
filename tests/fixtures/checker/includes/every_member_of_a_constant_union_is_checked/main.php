<?php
function load(bool $b): void {
    $file = $b ? 'present.php' : 'gone.php';
    require __DIR__ . '/lib/' . $file;
}

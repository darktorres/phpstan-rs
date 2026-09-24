<?php
if (file_exists(__DIR__ . '/optional.php')) {
    require __DIR__ . '/optional.php';
}
function f(): void {
    if (is_readable(__DIR__ . '/config.local.php')) {
        include __DIR__ . '/config.local.php';
    }
}
$cb = function () {
    if (is_file('local.php')) {
        include 'local.php';
    }
};

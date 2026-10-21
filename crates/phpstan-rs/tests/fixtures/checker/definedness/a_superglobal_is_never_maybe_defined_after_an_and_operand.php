<?php
function f(): void {
    $ok = is_string($_GET['a'] ?? null) && $z = 1;
    echo count($_GET);
    echo $z;
}

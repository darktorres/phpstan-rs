<?php
[$a, $b] = [1, 2];
foreach ([[1, 2]] as [$c, $d]) {
    echo $a, $b, $c, $d;
}

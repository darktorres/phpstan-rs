<?php
$arr = [1, 2];
[&$a, $b] = $arr;
var_dump($a, $b,);
$x = <<<EOT
    hello
    EOT;
echo $x;

<?php
function f(array &$list, string ...$rest): void {
    eval('1;');
    isset($list[0]);
    empty($list);
    compact('list');
    $x = @file('a');
    switch ($x) { default: break; }
    $y = $x ?: 1;
    $z = $x == 1;
    $list[] = 1;
    $o = `ls`;
    $g = $_GET['q'];
    $v = $$x;
    $m = $x->$y;
    $n = &$x;
    f(list: $list);
    exit;
}

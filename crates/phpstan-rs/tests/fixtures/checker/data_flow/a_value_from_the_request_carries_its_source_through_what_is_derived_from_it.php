<?php
use function PhpstanRs\Testing\assertFlow;

function f(): void {
    $page = (int) ($_GET['page'] ?? 1);
    assertFlow('env:$_GET', $_GET['page']);
    assertFlow('env:$_GET', $page);
    assertFlow('env:$_GET, env:$_POST, overflow:*@8', $page * (int) $_POST['size']);
    assertFlow('env:$_GET', "page {$page}");
    assertFlow('env:$_GET', trim($_GET['q'] ?? ''));
    assertFlow('', strlen($_GET['q'] ?? ''));
    assertFlow('', $_GET['page'] === '1');
    assertFlow('', 5 * 3);
    $n = $page;
    $n++;
    assertFlow('env:$_GET, overflow:++@15', $n);
    $m = rand(0, 1) ? $page : 7;
    assertFlow('env:$_GET', $m);
    assertFlow('global:$_SESSION', $_SESSION['id'] ?? 0);
    assertFlow('env:$_SERVER', explode(',', $_SERVER['HTTP_ACCEPT'] ?? ''));
}

function g(): void {
    $s = rand();
    $t = rand();
    for ($i = 0; $i < 3; $i++) {
        $s = $t;
        $t = (int) ($_GET['x'] ?? 0);
    }
    assertFlow('env:$_GET', $s);
}

function collected(): void {
    $ids = [];
    foreach ([1, 2, 3] as $i) {
        $ids[] = (int) ($_GET['id' . $i] ?? 0);
    }
    assertFlow('env:$_GET', $ids);
}

function h(): void {
    $t = rand();
    if (rand(0, 1)) {
        $t = (int) $_GET['x'];
    }
    assertFlow('env:$_GET', $t);
}

f();
g();
h();
collected();

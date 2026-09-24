<?php
function f(int $ts, string $when): void {
    time();
    microtime(true);
    date('Y-m-d');
    date('Y-m-d', $ts);
    date('Y-m-d', null);
    getdate();
    mktime(0, 0, 0, 1, 1, 2024);
    mktime(0);
    strtotime('+1 day');
    strtotime('+1 day', $ts);
    strtotime('2024-01-01');
    strtotime($when);
    new \DateTimeImmutable();
    new \DateTimeImmutable('next monday');
    new \DateTimeImmutable('2024-01-01');
    new \DateTime('10:30');
    date_create();
    \DateTimeImmutable::createFromFormat('Y-m-d', '2024-01-01');
    \DateTimeImmutable::createFromFormat('!Y-m-d', '2024-01-01');
    \DateTimeImmutable::createFromFormat('U', '1700000000');
    $_SERVER['REQUEST_TIME'];
    new \ArrayObject();
}

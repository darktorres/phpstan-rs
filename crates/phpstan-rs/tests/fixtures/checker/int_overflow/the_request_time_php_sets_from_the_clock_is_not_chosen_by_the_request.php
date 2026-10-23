<?php
declare(strict_types=1);

function expires(int $at): int
{
    return $at;
}

function started(): int
{
    return expires((int) $_SERVER['REQUEST_TIME'] - 3600);
}

function started_precisely(): int
{
    return expires((int) $_SERVER['REQUEST_TIME_FLOAT'] - 3600);
}

function sent(): int
{
    return expires((int) $_SERVER['HTTP_EXPIRES'] - 3600);
}

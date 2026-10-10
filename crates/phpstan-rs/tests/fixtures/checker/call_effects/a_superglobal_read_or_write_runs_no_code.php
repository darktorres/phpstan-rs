<?php
namespace CallEffects\Superglobals;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?int $id = null;
}

function readsARequestValue(): string
{
    return $_GET['name'] ?? '';
}

function writesASessionValue(): void
{
    $_SESSION['seen'] = true;
    unset($_COOKIE['token']);
}

function aSuperglobalReadKeepsAPropertyFact(Box $box): void
{
    if ($box->id !== null) {
        readsARequestValue();
        assertType('int', $box->id);
    }
}

function aSuperglobalWriteKeepsAPropertyFact(Box $box): void
{
    if ($box->id !== null) {
        writesASessionValue();
        assertType('int', $box->id);
    }
}

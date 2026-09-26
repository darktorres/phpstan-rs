<?php
class Runner
{
    public function run(): void {}
}

function invoke(Runner $r, string $m): int
{
    $r->{$m}();
    return 1;
}

/** @phpstan-pure */
function pureInvoke(Runner $r, string $m): int
{
    $r->{$m}();
    return 1;
}

function main(Runner $r): void
{
    invoke($r, 'run');
}

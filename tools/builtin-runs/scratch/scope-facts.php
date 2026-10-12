<?php
// forward_static_call and forward_static_call_array need a class scope: they are called from a method of a
// fixture class here, with a callable that counts its own calls. The fact is the callable's call, recorded at
// the position of the callable. The fixture class is part of this experiment only; the table never names it.
// Run: php scope-facts.php OUT.tsv

final class ScopeFixture
{
    public static int $calls = 0;

    public function forward(callable $callback, mixed ...$args): mixed
    {
        return forward_static_call($callback, ...$args);
    }

    public function forwardArray(callable $callback, array $args): mixed
    {
        return forward_static_call_array($callback, $args);
    }
}

$out = fopen($argv[1], 'w');
$callable = static function (mixed ...$args): int {
    ScopeFixture::$calls++;
    return 0;
};
$fixture = new ScopeFixture();

ScopeFixture::$calls = 0;
$fixture->forward($callable, 1);
fwrite($out, 'forward_static_call' . "\t0\t" . (ScopeFixture::$calls > 0 ? 'calls' : '-') . "\n");

ScopeFixture::$calls = 0;
$fixture->forwardArray($callable, [1]);
fwrite($out, 'forward_static_call_array' . "\t0\t" . (ScopeFixture::$calls > 0 ? 'calls' : '-') . "\n");
fclose($out);
echo "done\n";

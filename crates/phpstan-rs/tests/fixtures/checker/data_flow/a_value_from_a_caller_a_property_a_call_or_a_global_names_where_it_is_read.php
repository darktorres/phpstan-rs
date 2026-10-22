<?php

namespace App;

use function PhpstanRs\Testing\assertFlow;

class C
{
    public int $n = 0;
    public static int $s = 0;

    public function m(int $x): int|float
    {
        assertFlow('param:App\C::m::$x', $x);
        assertFlow('property:App\C::$n', $this->n);
        assertFlow('property:App\C::$s', self::$s);
        $this->n = (int) $_GET['n'];
        assertFlow('env:$_GET', $this->n);
        assertFlow('call:App\f()', f(1));
        assertFlow('call:->m()', $x > 0 ? $this->m(0) : 0);
        assertFlow('call:App\C::sm()', self::sm());
        assertFlow('', strlen('abc'));
        assertFlow('', (new \DateTime())->getTimestamp());
        $double = fn (int $y) => assertFlow('param:{closure}::$y', $y) * 2;
        $sum = array_sum(array_map(fn (int $v) => assertFlow('env:$_GET', $v), [(int) $_GET['v']]));
        return $double($sum);
    }

    public static function sm(): int
    {
        return 1;
    }
}

function f(int $a): int
{
    global $g;
    static $label = 'a';
    assertFlow('global:$g', $g);
    assertFlow('static:App\f::$label', $label);
    assertFlow('global:$_SESSION', $_SESSION['id'] ?? 0);
    return $a;
}

$g = 1;
$_SESSION = [];
$_GET = ['n' => '3', 'v' => '4'];
echo (new C())->m(1), "\n";

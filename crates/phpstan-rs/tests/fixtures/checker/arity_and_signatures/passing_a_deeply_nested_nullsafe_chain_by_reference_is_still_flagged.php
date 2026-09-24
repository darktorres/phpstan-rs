<?php
            class Inner34 { public int $value = 0; }
            class Outer34 { public ?Inner34 $inner = null; }
            function setValue34(int &$x): void { $x = 1; }
            function test(Outer34 $o): void { setValue34($o->inner?->value); }
        
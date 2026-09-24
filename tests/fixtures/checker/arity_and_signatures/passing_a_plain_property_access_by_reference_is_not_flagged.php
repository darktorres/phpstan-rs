<?php
            class Box31 { public int $value = 0; }
            function setValue31(int &$x): void { $x = 1; }
            function test(Box31 $b): void { setValue31($b->value); }
        
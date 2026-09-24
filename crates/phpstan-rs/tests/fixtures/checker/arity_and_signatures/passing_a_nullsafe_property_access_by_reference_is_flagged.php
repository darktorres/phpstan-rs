<?php
            class Box30 { public int $value = 0; }
            function setValue30(int &$x): void { $x = 1; }
            function test(?Box30 $b): void { setValue30($b?->value); }
        
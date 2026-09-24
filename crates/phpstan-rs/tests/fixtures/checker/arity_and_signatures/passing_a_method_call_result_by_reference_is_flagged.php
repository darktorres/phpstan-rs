<?php
            class Box32 { public function getValue(): int { return 1; } }
            function setValue32(int &$x): void { $x = 1; }
            function test(Box32 $b): void { setValue32($b->getValue()); }
        
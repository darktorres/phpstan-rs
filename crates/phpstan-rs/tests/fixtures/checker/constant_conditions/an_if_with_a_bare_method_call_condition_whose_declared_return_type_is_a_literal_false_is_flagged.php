<?php
        final class Foo {
            /** @return false */
            public function neverTrue(): bool { return false; }
        }
        function f(Foo $foo): void { if ($foo->neverTrue()) { echo 1; } }
        
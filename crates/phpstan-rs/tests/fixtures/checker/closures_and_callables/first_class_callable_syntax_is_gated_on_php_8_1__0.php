<?php
        class Fcc {
            public function m(): void {}
            public static function s(): void {}
        }
        function f(Fcc $o): void {
            $a = strlen(...);
            $b = $o->m(...);
            $c = Fcc::s(...);
        }
    
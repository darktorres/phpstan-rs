<?php
            interface Countable2 { public function count2(): int; }
            interface Nameable { public function name(): string; }
            function f(Countable2&Nameable $x): void { $x->totallyMissingMethod(); }
        
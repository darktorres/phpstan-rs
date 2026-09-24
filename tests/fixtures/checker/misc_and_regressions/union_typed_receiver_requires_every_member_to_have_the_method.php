<?php
            class A { public function foo(): void {} }
            class B {}
            function f(A|B $x): void { $x->foo(); }
        
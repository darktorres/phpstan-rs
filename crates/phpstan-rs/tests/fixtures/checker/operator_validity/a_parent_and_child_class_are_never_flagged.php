<?php
            class Base {}
            class Derived extends Base {}
            function f(Base $base, Derived $d): void {
                if ($base === $d) { echo 1; }
            }
        
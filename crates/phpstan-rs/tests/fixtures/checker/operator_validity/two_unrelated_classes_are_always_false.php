<?php
            class Foo {}
            class Bar {}
            function f(Foo $a, Bar $b): void {
                if ($a === $b) { echo 1; }
            }
        
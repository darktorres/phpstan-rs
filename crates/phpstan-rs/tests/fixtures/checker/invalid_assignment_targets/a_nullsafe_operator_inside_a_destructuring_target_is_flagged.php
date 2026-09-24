<?php
            class Foo { public ?Foo $bar = null; }
            function test(?Foo $f, array $x) {
                [$f?->bar] = $x;
            }
        
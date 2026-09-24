<?php
            class Foo { public ?Foo $bar = null; }
            function test(?Foo $f) {
                $x = $f?->bar;
            }
        
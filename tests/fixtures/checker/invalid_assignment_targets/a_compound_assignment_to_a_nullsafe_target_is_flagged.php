<?php
            class Foo { public int $bar = 0; }
            function test(?Foo $f) {
                $f?->bar += 1;
            }
        
<?php
            class Foo { public ?Foo $bar = null; }
            function test(?Foo $f) {
                $f?->bar = new Foo();
            }
        
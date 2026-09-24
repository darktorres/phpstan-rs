<?php
            class Foo { public ?Foo $bar = null; public ?Foo $baz = null; }
            function test(?Foo $f) {
                $f?->bar->baz = new Foo();
            }
        
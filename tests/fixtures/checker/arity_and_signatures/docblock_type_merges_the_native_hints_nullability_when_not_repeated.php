<?php
            class Foo {
                /** @param int $x */
                public function bar(?int $x = null): void {}
            }
            function test(Foo $f): void { $f->bar(null); }
        
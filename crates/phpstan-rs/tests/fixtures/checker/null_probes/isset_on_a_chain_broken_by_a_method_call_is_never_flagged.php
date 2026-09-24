<?php
            class Foo {
                /** @return array{key: int} */
                public function getArr(): array { return ['key' => 1]; }
            }
            function test(Foo $f): void {
                if (isset($f->getArr()['key'])) {
                }
            }
        
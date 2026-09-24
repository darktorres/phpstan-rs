<?php
            class Other {
                public function helper(): void {}
            }
            class Foo {
                public function run(): void {
                    $x = new Other();
                    $x->helper();
                }
                private function helper(): void {}
            }
        
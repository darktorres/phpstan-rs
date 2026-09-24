<?php
            class Single {
                private function __construct() {}
                public static function make(): self { return new self(); }
            }
            class Guarded {
                protected function __construct() {}
            }
            class GuardedChild extends Guarded {
                public static function make(): self { return new self(); }
            }
            function f(): void {
                $a = new Single();
                $b = new Guarded();
                $c = new GuardedChild();
            }
        
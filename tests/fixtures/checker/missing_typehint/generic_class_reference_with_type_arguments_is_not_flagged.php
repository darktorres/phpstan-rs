<?php
            /**
             * @template T
             */
            class Box {}
            class C {
                /**
                 * @param Box<int> $b
                 */
                public function foo(Box $b): void {}
            }
        
<?php
            /**
             * @template T
             */
            class Box {
                /** @var T */
                public $value;
            }
            class C {
                /** @var array<int> */
                public const LIMITS = [1, 2, 3];

                private int $count = 0;

                /**
                 * @param Box<int> $b
                 * @return array<int>
                 */
                public function foo(Box $b): array {
                    return [$this->count];
                }
            }
        
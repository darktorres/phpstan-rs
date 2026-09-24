<?php
            class C {
                /** @var array{a?: int, b?: string} */
                private array $opts = ['a' => 1, 'b' => 'x'];

                /** @param array{a?: int, b?: string} $x */
                public function __construct(array $x) {
                    $this->opts = array_merge($this->opts, $x);
                }
            }
        
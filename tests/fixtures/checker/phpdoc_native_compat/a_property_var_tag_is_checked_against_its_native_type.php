<?php
            interface Shape {}
            class Circle implements Shape {}
            class Holder {
                /** @var string */
                public int $count = 0;
                /** @var Shape */
                public Circle $circle;
                /** @var list<int> */
                public array $ids = [];
                public function __construct(
                    /** @var non-empty-string */
                    public string $name,
                    /** @var int */
                    public string $label,
                ) {}
            }
        
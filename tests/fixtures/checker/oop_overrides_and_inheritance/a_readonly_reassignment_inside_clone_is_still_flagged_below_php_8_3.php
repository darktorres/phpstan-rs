<?php
            class Foo {
                public function __construct(public readonly array $items) {}
                public function __clone(): void {
                    $this->items = [...$this->items];
                }
            }
        
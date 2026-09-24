<?php
            class Foo {
                private string $x;
                public function get(): string {
                    $this->x ??= "default";
                    return $this->x;
                }
            }
        
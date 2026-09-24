<?php
            class Config1 {
                /** @var list<string> */
                public array $names = ['a'] {
                    set(array $value) {
                        $this->names = array_values($value);
                    }
                }
            }
            function f(Config1 $c, array $raw): void {
                $c->names = $raw;
            }
        
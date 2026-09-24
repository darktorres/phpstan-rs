<?php
            class Config2 {
                public int $level = 0 {
                    set(int $value) {
                        $this->level = $value;
                    }
                }
            }
            function f(Config2 $c, string $bad): void {
                $c->level = $bad;
            }
        
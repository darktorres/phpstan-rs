<?php
            class Widget {
                private int $count = 0;
                public function bump(): void {
                    $this->count = $this->count + 1;
                }
                public function get(): int {
                    return $this->count;
                }
            }
        
<?php
            class Widget {
                /** @phpstan-always-read */
                private int $count;
                public function bump(): void {
                    $this->count = 1;
                }
            }
        
<?php
            class Money {
                public function add(self $other): void {
                    $other->helper();
                }
                private function helper(): void {}
            }
        
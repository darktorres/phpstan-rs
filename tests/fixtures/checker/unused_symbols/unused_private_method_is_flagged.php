<?php
            class Service {
                public function run(): int {
                    return $this->helper();
                }
                private function helper(): int { return 1; }
                private function deadCode(): int { return 2; }
            }
        
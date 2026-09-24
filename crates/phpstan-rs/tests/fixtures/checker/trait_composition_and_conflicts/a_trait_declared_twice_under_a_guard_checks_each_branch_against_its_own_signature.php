<?php
            if (true) {
                trait Adder {
                    public function add(): bool { return true; }
                }
            } else {
                trait Adder {
                    public function add(): void {}
                }
            }
        
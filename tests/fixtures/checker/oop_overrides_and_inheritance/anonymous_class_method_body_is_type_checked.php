<?php
            function needsInt(int $x): int { return $x; }
            interface Greeter { public function greet(): string; }
            function make() {
                return new class implements Greeter {
                    public function greet(): string {
                        return needsInt("wrong");
                    }
                };
            }
        
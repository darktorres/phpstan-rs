<?php
            function uses($class) {}
            function it($description, $callback) {}
            class RealTestBase {
                protected function helper(): int { return 1; }
            }
            uses(RealTestBase::class);
            it('does something', function (): int {
                return $this->helper();
            });
        
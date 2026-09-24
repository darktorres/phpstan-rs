<?php
            function uses($class) {}
            function it($description, $callback) {}
            class RealTestBase {
                public function realMethod(): int { return 1; }
            }
            uses(RealTestBase::class);
            it('does something', function (): int {
                return $this->totallyUndefinedMethod();
            });
        
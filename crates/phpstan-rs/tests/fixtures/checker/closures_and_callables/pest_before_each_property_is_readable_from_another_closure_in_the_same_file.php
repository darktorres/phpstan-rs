<?php
            function uses($class) {}
            function beforeEach($callback) {}
            function it($description, $callback) {}
            class RealTestBase {}
            uses(RealTestBase::class);
            beforeEach(function (): void { $this->root = 'x'; });
            it('does something', function (): void {
                echo $this->root;
            });
        
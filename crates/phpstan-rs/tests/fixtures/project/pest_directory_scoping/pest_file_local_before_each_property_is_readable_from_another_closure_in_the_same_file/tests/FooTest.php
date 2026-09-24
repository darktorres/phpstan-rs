<?php
            function beforeEach($callback) {}
            function it($description, $callback) {}
            beforeEach(function (): void { $this->root = 'x'; });
            it('works', function (): void { echo $this->root; });
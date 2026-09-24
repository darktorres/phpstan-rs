<?php
            function afterEach($callback) {}
            uses()
                ->beforeEach(function (): void { $this->root = 'x'; })
                ->afterEach(function (): void {})
                ->in('Unit');
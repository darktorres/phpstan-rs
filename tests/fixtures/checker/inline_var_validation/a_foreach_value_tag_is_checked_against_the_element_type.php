<?php
            /** @param list<int> $xs */
            function f(array $xs): void {
                /** @var string $x */
                foreach ($xs as $x) {}
                /** @var int $y */
                foreach ([1, 2] as $y) {}
            }
        
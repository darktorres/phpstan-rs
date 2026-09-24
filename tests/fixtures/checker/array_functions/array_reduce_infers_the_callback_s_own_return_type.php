<?php
            function needsInt(int $n): void {}
            function needsString(string $s): void {}
            $sum = array_reduce([1, 2, 3], fn($carry, $item) => $carry + $item, 0);
            needsInt($sum);
            needsString($sum);
        
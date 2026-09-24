<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function test(array $untyped): void { needsInts($untyped); }
        
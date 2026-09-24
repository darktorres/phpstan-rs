<?php
            function needsIterable(iterable $x): void {}
            function test(mixed $x): void {
                if (is_iterable($x)) {
                    needsIterable($x);
                }
            }
        
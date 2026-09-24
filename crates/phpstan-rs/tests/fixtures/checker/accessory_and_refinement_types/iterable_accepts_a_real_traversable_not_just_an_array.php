<?php
            function f(iterable $items): void {
                foreach ($items as $item) { echo $item; }
            }
            function test(): void {
                f(new ArrayIterator([1, 2, 3]));
                f([1, 2, 3]);
            }
        
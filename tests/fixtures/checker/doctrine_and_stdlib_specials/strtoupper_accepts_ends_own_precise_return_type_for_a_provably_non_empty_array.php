<?php
            /** @param non-empty-array<string> $items */
            function f(array $items): string {
                return strtoupper(end($items));
            }
        
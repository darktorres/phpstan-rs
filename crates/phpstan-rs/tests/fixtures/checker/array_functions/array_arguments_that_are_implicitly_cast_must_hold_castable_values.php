<?php
            class Plain {}
            class Named { public function __toString(): string { return ''; } }
            /**
             * @param list<array<int>> $nested
             * @param list<Plain> $plains
             * @param list<Named> $named
             * @param list<int> $ints
             */
            function f(array $nested, array $plains, array $named, array $ints): void {
                implode(',', $nested);
                implode(',', $named);
                implode($ints);
                array_sum($nested);
                array_sum($ints);
                array_diff($ints, $plains);
                array_combine($plains, $ints);
                array_unique($plains);
                sort($plains);
                sort($plains, SORT_NUMERIC);
                sort($named, SORT_STRING);
            }
        
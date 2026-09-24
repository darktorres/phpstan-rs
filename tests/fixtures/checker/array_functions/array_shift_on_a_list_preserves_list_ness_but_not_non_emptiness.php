<?php
            /**
             * @param list<int> $x
             * @return list<int>
             */
            function f(array $x): array {
                array_shift($x);
                return $x;
            }
        
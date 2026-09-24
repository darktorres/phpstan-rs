<?php
            /** @param list<int>|null $x
             *  @return list<int>
             */
            function f($x): array {
                sort($x);
                return $x;
            }
        
<?php
            /** @param list<string> $x
             *  @return non-empty-list<string>
             */
            function f(array $x): array {
                $x[] = 'y';
                return $x;
            }
        
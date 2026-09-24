<?php
            /** @param list<string> $x
             *  @return list<string>
             */
            function f(array $x, string $key): array {
                $x[$key] = 'y';
                return $x;
            }
        
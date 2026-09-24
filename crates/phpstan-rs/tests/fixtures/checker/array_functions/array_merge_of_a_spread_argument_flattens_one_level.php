<?php
            /** @param array<string, list<int>> $byWord
             *  @return list<int>
             */
            function f(array $byWord): array {
                return array_merge(...array_values($byWord));
            }
        
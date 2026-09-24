<?php
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingFilter($x): array {
                return array_filter($x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingCombine($x): array {
                return array_combine($x, $x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingUnique($x): array {
                return array_unique($x);
            }
        
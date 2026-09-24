<?php
            /** @param list<int> $x
             *  @return list<int>
             */
            function sliceDefault($x): array {
                return array_slice($x, 1);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function reverseDefault($x): array {
                return array_reverse($x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function slicePreserveKeys($x): array {
                return array_slice($x, 1, null, true);
            }
        
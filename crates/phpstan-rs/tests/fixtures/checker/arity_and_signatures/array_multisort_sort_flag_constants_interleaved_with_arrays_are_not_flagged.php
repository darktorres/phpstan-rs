<?php
            function test(array $q, array $i): void {
                array_multisort($q, SORT_DESC, SORT_NUMERIC, $i, SORT_ASC, SORT_NUMERIC);
            }
        
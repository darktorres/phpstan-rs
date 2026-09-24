<?php
            /** @return non-empty-list<int> */
            function f(): array {
                return array_map(intval(...), explode(',', '1,2,3'));
            }
        
<?php
            /** @param list<string> $xs */
            function needsList(array $xs): void {}
            /** @param array<string, callable> $filters */
            function f(array $filters): void {
                needsList(array_keys($filters));
            }
        
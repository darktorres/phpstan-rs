<?php
        /**
         * @param array<string, int> $named
         * @param list<int> $list
         * @param array<int|string, int> $mixed
         */
        function f(array $named, array $list, array $mixed): array {
            return [...$named, ...$list, ...$mixed];
        }
    
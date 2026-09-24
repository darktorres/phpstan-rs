<?php
            /**
             * @param list<string> $clauses
             * @param list<string> $values
             * @return array{0: list<string>, 1: list<string>}
             */
            function f(array $clauses, array $values): array {
                array_splice($values, 0, 0, ['tsv_search @@ to_tsquery(?)']);
                return [$clauses, $values];
            }
        
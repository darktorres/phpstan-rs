<?php
            /** @return list<int> */
            function build(array $rows): array {
                $x = [];
                foreach ($rows as $row) {
                    if ($row) {
                        $x[] = $row;
                    }
                }
                return array_filter($x, is_int(...));
            }
        
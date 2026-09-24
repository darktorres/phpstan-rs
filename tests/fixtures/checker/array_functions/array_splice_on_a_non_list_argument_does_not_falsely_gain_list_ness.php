<?php
            /** @return list<string> */
            function f(): array {
                $x = ['a' => 'foo'];
                array_splice($x, 0, 1, ['bar']);
                return $x;
            }
        
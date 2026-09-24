<?php
            /** @return list<string> */
            function f(): array {
                $x = ['a' => 'foo'];
                array_push($x, 'bar');
                return $x;
            }
        
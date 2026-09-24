<?php
            /** @return list<string> */
            function f(string $head): array {
                $chain = [];
                array_unshift($chain, $head);
                return $chain;
            }
        
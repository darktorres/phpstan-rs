<?php
            /** @return list<string> */
            function split(string $pattern, string $subject): array {
                $list = preg_split($pattern, $subject, -1, PREG_SPLIT_NO_EMPTY);
                if ($list === false) {
                    throw new Exception('failed');
                }
                return $list;
            }
        
<?php
            /** @param list<int> $x */
            function wantsIntList(array $x): void {}
            function f(string $pattern, string $subject): void {
                $list = preg_split($pattern, $subject);
                if ($list === false) {
                    throw new Exception('failed');
                }
                wantsIntList($list);
            }
        
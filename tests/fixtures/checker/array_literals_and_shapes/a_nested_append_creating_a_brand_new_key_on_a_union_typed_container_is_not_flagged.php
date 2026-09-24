<?php
            function test(bool $cond): void {
                if ($cond) {
                    $info = ['a' => 1];
                } else {
                    $info = ['b' => 2];
                }
                $info['updates'][] = 'x';
            }
        
<?php
            function test(bool $cond): void {
                $info = ['a' => 1];
                if ($cond) {
                    $info['b'] = 'x';
                } else {
                    $info['c'] = 'y';
                }
                $info['file_extensions'] = [];
                $info['file_extensions']['jpg'] = 3;
                echo $info['file_extensions']['jpg'];
            }
        
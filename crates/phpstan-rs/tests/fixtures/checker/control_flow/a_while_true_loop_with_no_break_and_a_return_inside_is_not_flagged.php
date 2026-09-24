<?php
            function f(): string {
                while (true) {
                    $key = 'x';
                    if ($key !== 'y') {
                        return $key;
                    }
                }
            }
        
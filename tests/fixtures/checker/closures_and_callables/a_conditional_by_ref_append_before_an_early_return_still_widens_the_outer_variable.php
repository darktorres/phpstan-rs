<?php
            function fakeHttp(callable $cb): void {}

            function test(bool $cond): void {
                $calls = [];
                fakeHttp(function () use (&$calls, $cond) {
                    if ($cond) {
                        $calls[] = 'a';

                        return 1;
                    }

                    return 2;
                });

                echo $calls[0];
            }
        
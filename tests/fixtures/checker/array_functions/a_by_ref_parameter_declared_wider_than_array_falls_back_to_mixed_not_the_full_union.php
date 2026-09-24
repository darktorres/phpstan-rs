<?php
            /**
             * @param array<int|string, mixed> $array
             * @return array<int|string, string>
             */
            function test(array $array): array {
                array_walk($array, function (mixed &$value): void {
                    $value = (string) $value;
                });
                return $array;
            }
        
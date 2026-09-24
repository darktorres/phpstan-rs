<?php
            /**
             * @param array<int, string> $byInt
             * @param array<string, int> $byName
             * @param list<int> $list
             */
            function f(array $byInt, array $byName, array $list): void {
                echo $byInt['name'];
                echo $byInt[3];
                echo $byInt['3'];
                echo $byName[0];
                echo $byName['x'];
                echo $list[-1];
                echo $list['k'];
                echo $list[0];
                $empty = [];
                echo $empty['gone'];
            }
        
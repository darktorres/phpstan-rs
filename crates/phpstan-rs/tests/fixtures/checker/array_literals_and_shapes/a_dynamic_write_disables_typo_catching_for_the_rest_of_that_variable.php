<?php
            function test(int $k): void {
                $x = [];
                $x['a'] = 1;
                $x[$k] = 2;
                echo $x['typo'];
            }
        
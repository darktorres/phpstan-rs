<?php
            function test(): void {
                $x = [];
                $x['a'] = 1;
                $x[] = 2;
                echo $x['typo'];
            }
        
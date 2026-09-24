<?php
            function test(): void {
                $f = function (int $x): void {};
                $f('not an int');
            }
        
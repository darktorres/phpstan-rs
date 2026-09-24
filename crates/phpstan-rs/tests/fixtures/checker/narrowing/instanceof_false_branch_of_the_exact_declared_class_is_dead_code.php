<?php
            class Cat {}
            function needsInt(int $n): void {}
            function handle(Cat $c): void {
                if (! ($c instanceof Cat)) {
                    needsInt($c);
                }
            }
        
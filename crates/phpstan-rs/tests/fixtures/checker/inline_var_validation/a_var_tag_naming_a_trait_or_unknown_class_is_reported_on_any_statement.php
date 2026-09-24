<?php
            trait T {}
            function f($x): void {
                /** @var T $x */
                echo $x;
                /** @var Missing $x */
                return;
            }
        
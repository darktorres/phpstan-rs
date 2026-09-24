<?php
            function needsString(string $s): void {}
            function f($x): void {
                needsString(get_class($x));
            }
        
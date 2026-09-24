<?php
            function alwaysTrue(): true { return true; }
            function f(bool $x) {
                $x or alwaysTrue();
            }
        
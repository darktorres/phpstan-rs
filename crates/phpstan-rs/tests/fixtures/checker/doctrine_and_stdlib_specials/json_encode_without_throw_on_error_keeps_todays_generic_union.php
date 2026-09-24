<?php
            function f($x): string {
                return json_encode($x);
            }
            function g($x): string {
                return json_encode($x, JSON_PRETTY_PRINT);
            }
        
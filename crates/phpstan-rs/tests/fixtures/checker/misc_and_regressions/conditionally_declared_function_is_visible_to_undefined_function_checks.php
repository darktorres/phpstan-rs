<?php
            if (!function_exists('polyfilled')) {
                function polyfilled() {}
            }
            function f() { return polyfilled(); }
        
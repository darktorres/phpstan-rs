<?php
            define('MY_HEX_TAG', 1);
            define('MY_HEX_APOS', 4);
            function f($flags) {}
            f(MY_HEX_TAG | MY_HEX_APOS);
            f(MY_HEX_TAG);
        
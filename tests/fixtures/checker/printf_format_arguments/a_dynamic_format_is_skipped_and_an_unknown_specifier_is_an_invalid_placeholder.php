<?php
            function f(string $fmt) {
                sprintf($fmt, "whatever");
                sprintf("%v", "x");
                sprintf('%1$s %s', "only one");
            }
        
<?php
            function test(): callable {
                return function () use ($_SERVER) {
                    return $_SERVER;
                };
            }
        
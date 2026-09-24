<?php
            $gen = function (bool $done): Generator {
                if ($done) {
                    return;
                }
                yield 1;
            };
        
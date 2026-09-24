<?php
            function gen(bool $done): Generator {
                if ($done) {
                    return;
                }
                yield 1;
            }
        
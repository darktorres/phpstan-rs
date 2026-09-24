<?php
            function test(?int $x): int {
                return $x ?? throw new \RuntimeException('bad');
            }
        
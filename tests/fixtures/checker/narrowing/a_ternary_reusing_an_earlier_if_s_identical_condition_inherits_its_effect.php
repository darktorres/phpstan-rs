<?php
            use function PHPStan\Testing\assertType;
            function f(bool $sendDetail): void {
                $news = null;
                if ($sendDetail) {
                    $news = "abc";
                }
                $result = $sendDetail ? $news : null;
                assertType("'abc'|null", $result);
            }
        
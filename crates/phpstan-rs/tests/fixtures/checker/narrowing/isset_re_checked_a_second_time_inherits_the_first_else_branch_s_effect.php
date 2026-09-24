<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $dbnow = null;
                if (isset($image_id)) {
                } else {
                    $dbnow = "now";
                }
                echo "unrelated code in between";
                if (isset($image_id)) {
                } else {
                    assertType("'now'", $dbnow);
                }
            }
        
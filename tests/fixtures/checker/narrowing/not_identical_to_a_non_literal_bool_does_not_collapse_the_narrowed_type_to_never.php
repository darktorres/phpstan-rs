<?php
            function test(?bool $a, bool $b): void {
                $update = [];
                if ($a !== null && $a !== $b) {
                    $update['flag'] = $a;
                }
                echo $update['flag'];
            }
        
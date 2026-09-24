<?php
            function f(array $data): void {
                $flag = false;
                foreach ($data as $v) {
                    if ($v) {
                        continue;
                    }
                    $flag = true;
                }
                if (!$flag) {
                    echo "unset";
                }
            }
        
<?php
            function f(string $path): void {
                $info = pathinfo($path);
                if (isset($info['dirname'])) {
                    echo $info['dirname'];
                }
            }
        
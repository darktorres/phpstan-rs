<?php
            function f(?ReflectionClass $r): void {
                if ($r === null || ($r = $r->getMethod('x')) || $r->isPublic()) {
                    echo "ok";
                }
            }
        
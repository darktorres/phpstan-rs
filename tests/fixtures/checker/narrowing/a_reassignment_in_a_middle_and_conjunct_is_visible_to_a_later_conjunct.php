<?php
            function f(ReflectionClass $r): void {
                if ($r->hasMethod('__call') && ($r = $r->getMethod('__call')) && $r->isPublic()) {
                    echo "ok";
                }
            }
        
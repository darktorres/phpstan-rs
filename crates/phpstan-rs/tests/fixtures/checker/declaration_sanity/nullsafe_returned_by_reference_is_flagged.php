<?php
            class Box { public ?Box $next = null; public int $v = 0; }
            function &byRef(?Box $b) {
                if ($b === null) { $z = 0; return $z; }
                return $b?->next;
            }
            function notByRef(?Box $b) { return $b?->next; }
            $f = fn &(?Box $b) => $b?->v;
            $g = function &(?Box $b) { $inner = function () use ($b) { return $b?->v; }; return $b->v; };
        
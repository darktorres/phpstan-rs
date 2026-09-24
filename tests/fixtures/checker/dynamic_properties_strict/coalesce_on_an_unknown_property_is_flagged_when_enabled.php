<?php
            class C {}
            function f(C $c): mixed {
                return $c->missing ?? 'default';
            }
        
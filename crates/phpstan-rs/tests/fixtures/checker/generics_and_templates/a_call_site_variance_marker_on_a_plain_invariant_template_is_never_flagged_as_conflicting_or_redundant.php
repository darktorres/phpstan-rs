<?php
            /** @template T */
            class Collection {}
            class Animal {}
            /** @param Collection<covariant Animal> $c */
            function f($c) {}
            /** @param Collection<contravariant Animal> $g */
            function g($g) {}
        
<?php
            /** @template-contravariant T */
            class Collection {}
            class Animal {}
            /** @param Collection<covariant Animal> $c */
            function f($c) {}
        
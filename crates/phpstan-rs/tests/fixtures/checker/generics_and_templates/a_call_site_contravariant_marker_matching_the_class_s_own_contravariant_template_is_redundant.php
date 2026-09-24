<?php
            /** @template-contravariant T */
            class Collection {}
            class Animal {}
            /** @param Collection<contravariant Animal> $c */
            function f($c) {}
        
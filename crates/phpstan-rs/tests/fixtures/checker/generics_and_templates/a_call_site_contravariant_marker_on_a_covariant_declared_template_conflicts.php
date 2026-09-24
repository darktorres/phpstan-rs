<?php
            /** @template-covariant T */
            class Collection {}
            class Animal {}
            /** @param Collection<contravariant Animal> $c */
            function f($c) {}
        
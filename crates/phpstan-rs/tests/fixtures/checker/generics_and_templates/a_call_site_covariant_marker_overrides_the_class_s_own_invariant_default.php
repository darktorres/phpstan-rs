<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<covariant Animal> $c */
            function needsAnimalCollection($c) {}
            /** @param Collection<Cat> $c */
            function f($c) {
                needsAnimalCollection($c);
            }
        
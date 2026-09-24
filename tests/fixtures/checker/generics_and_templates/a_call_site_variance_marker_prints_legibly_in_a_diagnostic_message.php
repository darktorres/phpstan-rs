<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<covariant Cat> $c */
            function needsCatCollection($c) {}
            /** @param Collection<Animal> $c */
            function f($c) {
                needsCatCollection($c);
            }
        
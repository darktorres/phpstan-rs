<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-contravariant T
             */
            class Comparator {}
            /** @param Comparator<Cat> $c */
            function needsCatComparator($c) {}
            /** @param Comparator<Animal> $c */
            function f($c) {
                needsCatComparator($c);
            }
        
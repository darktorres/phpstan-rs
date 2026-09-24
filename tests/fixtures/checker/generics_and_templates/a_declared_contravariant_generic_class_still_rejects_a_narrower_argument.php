<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-contravariant T
             */
            class Comparator {}
            /** @param Comparator<Animal> $c */
            function needsAnimalComparator($c) {}
            /** @param Comparator<Cat> $c */
            function f($c) {
                needsAnimalComparator($c);
            }
        
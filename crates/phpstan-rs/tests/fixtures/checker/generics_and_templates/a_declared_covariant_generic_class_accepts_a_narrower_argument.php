<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-covariant T
             */
            class Box {}
            /** @param Box<Animal> $b */
            function needsAnimalBox($b) {}
            /** @param Box<Cat> $b */
            function f($b) {
                needsAnimalBox($b);
            }
        
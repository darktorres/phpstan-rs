<?php
            class Animal {}
            class Cat extends Animal {}
            /**
             * @template-covariant T
             */
            class Box {}
            /** @param Box<Cat> $b */
            function needsCatBox($b) {}
            /** @param Box<Animal> $b */
            function f($b) {
                needsCatBox($b);
            }
        
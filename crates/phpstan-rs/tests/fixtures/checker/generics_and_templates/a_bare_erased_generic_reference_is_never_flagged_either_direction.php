<?php
            class Shape {}
            class Circle extends Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<Circle> $c */
            function needsCircleCollection($c) {}
            /** @param Collection $c */
            function f($c) {
                needsCircleCollection($c);
            }
        
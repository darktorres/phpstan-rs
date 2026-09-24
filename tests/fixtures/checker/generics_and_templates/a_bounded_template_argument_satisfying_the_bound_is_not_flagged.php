<?php
            interface Shape {}
            class Circle implements Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<Circle> $c */
            function f($c) {}
        
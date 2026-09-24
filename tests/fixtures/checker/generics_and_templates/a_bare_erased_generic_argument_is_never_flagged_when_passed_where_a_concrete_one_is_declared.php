<?php
            class Circle {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection $c */
            function needsAnyCollection($c) {}
            /** @param Collection<Circle> $c */
            function f($c) {
                needsAnyCollection($c);
            }
        
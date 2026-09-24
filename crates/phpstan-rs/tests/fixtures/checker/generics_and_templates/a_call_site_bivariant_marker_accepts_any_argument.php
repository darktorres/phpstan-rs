<?php
            class Whatever {}
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<*> $c */
            function needsAnyCollection($c) {}
            /** @param Collection<Whatever> $c */
            function f($c) {
                needsAnyCollection($c);
            }
        
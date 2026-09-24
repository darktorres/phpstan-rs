<?php
            /**
             * @template T
             */
            class Collection {}
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            /** @param Collection<int> $c */
            function needsIntCollection($c) {}
            function f(IntCollection $c) {
                needsIntCollection($c);
            }
        
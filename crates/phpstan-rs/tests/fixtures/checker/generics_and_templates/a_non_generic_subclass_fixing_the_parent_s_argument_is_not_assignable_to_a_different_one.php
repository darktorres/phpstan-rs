<?php
            /**
             * @template T
             */
            class Collection {}
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            /** @param Collection<string> $c */
            function needsStringCollection($c) {}
            function f(IntCollection $c) {
                needsStringCollection($c);
            }
        
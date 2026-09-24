<?php
            /** @template T */
            interface First {}
            /** @template K
             *   @template V
             */
            interface Second {}
            /**
             * @extends First<int>
             * @extends Second<string, bool>
             */
            interface Combined extends First, Second {}
        
<?php
            /** @template T */
            trait First {}
            /** @template K
             *   @template V
             */
            trait Second {}
            class Combined {
                /** @use First<int, string> */
                use First;
                /** @use Second<string> */
                use Second;
            }
        
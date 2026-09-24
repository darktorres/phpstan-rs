<?php
            function f(array $xs): void {
                goto nowhere;
                goto inside;
                foreach ($xs as $x) {
                    inside:
                    goto out;
                }
                out:
                unused:
                dup:
                dup:
                try {
                    goto fin;
                } finally {
                    fin:
                    goto out;
                }
            }
        
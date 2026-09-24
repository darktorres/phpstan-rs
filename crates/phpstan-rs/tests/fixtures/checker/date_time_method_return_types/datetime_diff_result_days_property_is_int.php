<?php
            /** @param int $n */
            function sink(int $n): void {}
            function f(\DateTime $a, \DateTime $b): void {
                $diff = $a->diff($b);
                sink($diff->days);
            }
        
<?php
            function sink(\DateInterval $i): void {}
            function f(\DateTime $a, \DateTime $b): void {
                sink($a->diff($b));
            }
        
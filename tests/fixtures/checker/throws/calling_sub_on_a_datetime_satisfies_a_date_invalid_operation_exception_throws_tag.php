<?php
            /** @throws \DateInvalidOperationException */
            function f(\DateTime $dt, \DateInterval $i): void {
                $dt->sub($i);
            }
        
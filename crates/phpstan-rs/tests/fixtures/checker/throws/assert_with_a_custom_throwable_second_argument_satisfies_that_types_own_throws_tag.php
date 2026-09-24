<?php
            /** @throws \LogicException */
            function f(bool $cond): void {
                assert($cond, new \LogicException('custom'));
            }
        
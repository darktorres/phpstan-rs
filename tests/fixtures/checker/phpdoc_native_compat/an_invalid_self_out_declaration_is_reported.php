<?php
            class Other {}
            class Thing {
                /** @phpstan-self-out Other */
                public function a(): void {}
                /** @phpstan-self-out self */
                public static function b(): void {}
                /** @phpstan-self-out string[optional] */
                public function c(): void {}
            }
        
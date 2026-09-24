<?php
            class Base {
                public static function make(): void {}
            }
            function f(): void {
                base::make();
            }
        
<?php
            class Svc {
                public function run(): void {}
                public static function make(): void { self::run(); }
                public function again(): void { self::run(); static::run(); }
            }
            class SvcChild extends Svc {
                public function go(): void { parent::run(); Svc::run(); }
            }
            class Unrelated {
                public function go(): void { Svc::run(); }
            }
            function outside(): void { Svc::run(); }
        
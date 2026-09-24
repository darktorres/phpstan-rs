<?php
            const DEBUG = false;
            define('MODE', 'dev');
            class Settings { const LEVEL = 3; }
            function f(): void {
                \PHPStan\Testing\assertType('bool', DEBUG);
                \PHPStan\Testing\assertType("'dev'|'prod'", MODE);
                \PHPStan\Testing\assertType('int', Settings::LEVEL);
                \PHPStan\Testing\assertType('string', FROM_ENV);
            }
        
<?php
            use function PHPStan\Testing\assertType;
            function run(): void {
                assertType("'apache'|'apache2handler'|'cgi'|'cli'|'cli-server'|'embed'|'fpm-fcgi'|'litespeed'|'phpdbg'|non-falsy-string", PHP_SAPI);
                assertType("'Windows'|'BSD'|'Darwin'|'Solaris'|'Linux'|'Unknown'", PHP_OS_FAMILY);
                assertType('int<50207, max>', PHP_VERSION_ID);
                assertType('4|8', PHP_INT_SIZE);
                assertType('resource', STDERR);
                if (PHP_SAPI !== 'cli') {
                    echo 'web';
                }
                if (PHP_EOL === "\n") {
                    echo 'unix';
                }
                if (DIRECTORY_SEPARATOR === '\\') {
                    echo 'windows';
                }
            }
        
<?php
            const DECLARED = 1;
            define('DEFINED_TOP', 2);
            function setup(): void { define('DEFINED_IN_FUNCTION', 3); }
            function f(): void {
                echo DECLARED, DEFINED_TOP, DEFINED_IN_FUNCTION, PHP_EOL, __DIR__, __CLASS__;
                echo NOPE;
                if (defined('GUARDED')) { echo GUARDED; }
                exit;
            }
        
<?php
            function test(): void {
                if (function_exists('totallyMadeUpFunctionName')) {
                    $cb = function () {
                        totallyMadeUpFunctionName();
                    };
                }
            }
        
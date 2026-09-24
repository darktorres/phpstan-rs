<?php
            function test(): void {
                if (function_exists('totallyMadeUpFunctionName')) {
                    echo "found";
                } else {
                    echo "not found";
                }
                totallyMadeUpFunctionName();
            }
        
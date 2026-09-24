<?php
            function test(callable $cb): void {
                call_user_func($cb, "anything");
            }
        
<?php
            function f(bool $b): string {
                return match ($b) {
                    true => "yes",
                    false => "no",
                };
            }
        
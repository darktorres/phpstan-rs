<?php
            function f(?string $name): string|false {
                return getenv($name);
            }
        
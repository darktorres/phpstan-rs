<?php
            function f(?string $name): void {
                $name === null || $resolved = $name;
                echo $resolved;
            }
        
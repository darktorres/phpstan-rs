<?php
            /** @return array{name: string, age: int} */
            function person(): array { return ["name" => "Al", "age" => 30]; }
            function test(): int {
                ["name" => $name, "age" => $age] = person();
                return $name;
            }
        
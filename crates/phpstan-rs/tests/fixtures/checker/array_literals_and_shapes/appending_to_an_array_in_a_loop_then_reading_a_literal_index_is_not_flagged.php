<?php
            function test(): void {
                $tagIds = [];
                foreach (['a', 'b', 'c'] as $name) {
                    $tagIds[] = strlen($name);
                }
                echo $tagIds[0];
                echo $tagIds[1];
                echo $tagIds[2];
            }
        
<?php
            function test(): void {
                $displayFilters = [];
                foreach (['words' => true, 'tags' => false] as $filtName => $filtConf) {
                    $displayFilters[$filtName] = $filtConf;
                }
                echo $displayFilters['words'];
            }
        
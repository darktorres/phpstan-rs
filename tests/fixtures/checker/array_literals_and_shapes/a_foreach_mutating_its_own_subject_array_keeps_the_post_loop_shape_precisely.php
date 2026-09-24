<?php
            function test(array $items, string $file): void {
                foreach ($items as $day => $data) {
                    $items[$day]['derivative'] = $file;
                }
                foreach ($items as $day => $data) {
                    echo $data['derivative'];
                }
            }
        
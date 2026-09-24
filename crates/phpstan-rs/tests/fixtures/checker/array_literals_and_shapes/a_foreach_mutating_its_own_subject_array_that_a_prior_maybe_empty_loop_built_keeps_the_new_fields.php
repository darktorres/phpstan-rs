<?php
            /** @param int[] $rows */
            function test(array $rows, string $file): void {
                $items = [];
                foreach ($rows as $period) {
                    $items[$period] = ['nb_images' => 0];
                }
                foreach ($items as $day => $data) {
                    $items[$day]['derivative'] = $file;
                }
                foreach ($items as $day => $data) {
                    echo $data['derivative'];
                }
            }
        
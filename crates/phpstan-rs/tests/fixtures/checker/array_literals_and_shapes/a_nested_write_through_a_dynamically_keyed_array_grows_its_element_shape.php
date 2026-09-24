<?php
            function test(int $period, string $file): void {
                $items = [];
                $items[$period] = ['nb_images' => 0];
                $items[$period]['derivative'] = $file;
                echo $items[$period]['derivative'];
            }
        
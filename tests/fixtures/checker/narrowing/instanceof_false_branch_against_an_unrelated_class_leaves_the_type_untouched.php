<?php
            class Cat {}
            class Dog {}
            function needsCat(Cat $c): void {}
            function handle(Cat|Dog $animal): void {
                if ($animal instanceof Dog) {
                    return;
                }
                needsCat($animal);
            }
        
<?php
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if (is_a($animal, 'Cat')) {
                    return useCat($animal);
                }
                return "";
            }
        
<?php
            abstract class Base {
                /** @throws \RuntimeException */
                abstract public function f(): void;
            }
            interface HasThrows {
                /** @throws \RuntimeException */
                public function g(): void;
            }
        
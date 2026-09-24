<?php
            class Foo {
                /** @phpstan-assert-if-false !null $this->parseError() */
                public function isValid(): bool { return true; }
            }
        
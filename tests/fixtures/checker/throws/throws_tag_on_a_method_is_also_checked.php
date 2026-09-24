<?php
            class NotAThrowable {}
            class Foo {
                /** @throws NotAThrowable */
                public function bar(): void {}
            }
        
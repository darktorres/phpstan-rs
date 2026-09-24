<?php
            class Foo {}
            function needsString(string $s): void {}
            function f(): void {
                needsString(Foo::class);
            }
        
<?php
            class Foo {}
            function test(): void {
                $h = Foo::totallyUndefinedStatic(...);
            }
        
<?php
            class Foo {}
            class Bar {}
            /** @param Closure(Foo): void $fn */
            function takesFooCallback($fn): void {}
            function test(): void {
                takesFooCallback(function (Bar $b): void {});
            }
        